#!/usr/bin/env bash

set -e

export MSYS_NO_PATHCONV=1

script=`cd $(dirname $0) && pwd`/`basename $0`
image=$1

docker_dir="`dirname $script`"
ci_dir="`dirname $docker_dir`"
src_dir="`dirname $ci_dir`"
root_dir="`dirname $src_dir`"

objdir=$root_dir/obj
dist=$objdir/build/dist

source "$ci_dir/shared.sh"

if [ -f "$docker_dir/$image/Dockerfile" ]; then
    if [ "$CI" != "" ]; then
      hash_key=/tmp/.docker-hash-key.txt
      rm -f "${hash_key}"
      echo $image >> $hash_key

      cat "$docker_dir/$image/Dockerfile" >> $hash_key
      # Look for all source files involves in the COPY command
      copied_files=/tmp/.docker-copied-files.txt
      rm -f "$copied_files"
      for i in $(sed -n -e 's/^COPY \(.*\) .*$/\1/p' "$docker_dir/$image/Dockerfile"); do
        # List the file names
        find "$docker_dir/$i" -type f >> $copied_files
      done
      # Sort the file names and cat the content into the hash key
      sort $copied_files | xargs cat >> $hash_key

      docker --version >> $hash_key
      cksum=$(sha512sum $hash_key | \
        awk '{print $1}')

      s3url="s3://$SCCACHE_BUCKET/docker/$cksum"
      url="https://$SCCACHE_BUCKET.s3.amazonaws.com/docker/$cksum"
      upload="aws s3 cp - $s3url"

      echo "Attempting to download $url"
      rm -f /tmp/rustci_docker_cache
      set +e
      retry curl -y 30 -Y 10 --connect-timeout 30 -f -L -C - -o /tmp/rustci_docker_cache "$url"
      loaded_images=$(docker load -i /tmp/rustci_docker_cache | sed 's/.* sha/sha/')
      set -e
      echo "Downloaded containers:\n$loaded_images"
    fi

    dockerfile="$docker_dir/$image/Dockerfile"
    if [ -x /usr/bin/cygpath ]; then
        context="`cygpath -w $docker_dir`"
        dockerfile="`cygpath -w $dockerfile`"
    else
        context="$docker_dir"
    fi
    retry docker \
      build \
      --rm \
      -t rust-ci \
      -f "$dockerfile" \
      "$context"

    if [ "$upload" != "" ]; then
      digest=$(docker inspect rust-ci --format '{{.Id}}')
      echo "Built container $digest"
      if ! grep -q "$digest" <(echo "$loaded_images"); then
        echo "Uploading finished image to $url"
        set +e
        docker history -q rust-ci | \
          grep -v missing | \
          xargs docker save | \
          gzip | \
          $upload
        set -e
      else
        echo "Looks like docker image is the same as before, not uploading"
      fi
      # Record the container image for reuse, e.g. by rustup.rs builds
      info="$dist/image-$image.txt"
      mkdir -p "$dist"
      echo "$url" >"$info"
      echo "$digest" >>"$info"
    fi
elif [ -f "$docker_dir/disabled/$image/Dockerfile" ]; then
    if isCI; then
        echo Cannot run disabled images on CI!
        exit 1
    fi
    # Transform changes the context of disabled Dockerfiles to match the enabled ones
    tar --transform 's#^./disabled/#./#' -C $docker_dir -c . | docker \
      build \
      --rm \
      -t rust-ci \
      -f "$image/Dockerfile" \
      -
else
    echo Invalid image: $image
    exit 1
fi

mkdir -p $HOME/.cargo
mkdir -p $objdir/tmp
mkdir -p $objdir/cores
mkdir -p /tmp/toolstate

args=
if [ "$SCCACHE_BUCKET" != "" ]; then
    args="$args --env SCCACHE_BUCKET"
    args="$args --env SCCACHE_REGION"
    args="$args --env AWS_ACCESS_KEY_ID"
    args="$args --env AWS_SECRET_ACCESS_KEY"
else
    mkdir -p $HOME/.cache/sccache
    args="$args --env SCCACHE_DIR=/sccache --volume $HOME/.cache/sccache:/sccache"
fi

# Run containers as privileged as it should give them access to some more
# syscalls such as ptrace and whatnot. In the upgrade to LLVM 5.0 it was
# discovered that the leak sanitizer apparently needs these syscalls nowadays so
# we'll need `--privileged` for at least the `x86_64-gnu` builder, so this just
# goes ahead and sets it for all builders.
args="$args --privileged"

# Things get a little weird if this script is already running in a docker
# container. If we're already in a docker container then we assume it's set up
# to do docker-in-docker where we have access to a working `docker` command.
#
# If this is the case (we check via the presence of `/.dockerenv`)
# then we can't actually use the `--volume` argument. Typically we use
# `--volume` to efficiently share the build and source directory between this
# script and the container we're about to spawn. If we're inside docker already
# though the `--volume` argument maps the *host's* folder to the container we're
# about to spawn, when in fact we want the folder in this container itself. To
# work around this we use a recipe cribbed from
# https://circleci.com/docs/2.0/building-docker-images/#mounting-folders to
# create a temporary container with a volume. We then copy the entire source
# directory into this container, and then use that copy in the container we're
# about to spawn. Finally after the build finishes we re-extract the object
# directory.
#
# Note that none of this is necessary if we're *not* in a docker-in-docker
# scenario. If this script is run on a bare metal host then we share a bunch of
# data directories to share as much data as possible. Note that we also use
# `LOCAL_USER_ID` (recognized in `src/ci/run.sh`) to ensure that files are all
# read/written as the same user as the bare-metal user.
if [ -f /.dockerenv ]; then
  docker create -v /checkout --name checkout alpine:3.4 /bin/true
  docker cp . checkout:/checkout
  args="$args --volumes-from checkout"
else
  args="$args --volume $root_dir:/checkout:ro"
  args="$args --volume $objdir:/checkout/obj"
  args="$args --volume $HOME/.cargo:/cargo"
  args="$args --volume $HOME/rustsrc:$HOME/rustsrc"
  args="$args --volume /tmp/toolstate:/tmp/toolstate"
  args="$args --env LOCAL_USER_ID=`id -u`"
fi

docker \
  run \
  --workdir /checkout/obj \
  --env SRC=/checkout \
  $args \
  --env CARGO_HOME=/cargo \
  --env DEPLOY \
  --env DEPLOY_ALT \
  --env CI \
  --env TF_BUILD \
  --env BUILD_SOURCEBRANCHNAME \
  --env TOOLSTATE_REPO_ACCESS_TOKEN \
  --env TOOLSTATE_REPO \
  --env TOOLSTATE_PUBLISH \
  --env CI_JOB_NAME="${CI_JOB_NAME-$IMAGE}" \
  --init \
  --rm \
  rust-ci \
  /checkout/src/ci/run.sh

if [ -f /.dockerenv ]; then
  rm -rf $objdir
  docker cp checkout:/checkout/obj $objdir
fi
