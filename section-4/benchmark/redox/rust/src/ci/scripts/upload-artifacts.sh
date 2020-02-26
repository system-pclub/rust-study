#!/bin/bash
# Upload all the artifacts to our S3 bucket. All the files inside ${upload_dir}
# will be uploaded to the deploy bucket and eventually signed and released in
# static.rust-lang.org.

set -euo pipefail
IFS=$'\n\t'

source "$(cd "$(dirname "$0")" && pwd)/../shared.sh"

upload_dir="$(mktemp -d)"

# Release tarballs produced by a dist builder.
if [[ "${DEPLOY-0}" -eq "1" ]] || [[ "${DEPLOY_ALT-0}" -eq "1" ]]; then
    dist_dir=build/dist
    if isLinux; then
        dist_dir=obj/build/dist
    fi
    rm -rf "${dist_dir}/doc"
    cp -r "${dist_dir}"/* "${upload_dir}"
fi

# CPU usage statistics.
cp cpu-usage.csv "${upload_dir}/cpu-${CI_JOB_NAME}.csv"

# Toolstate data.
if [[ -n "${DEPLOY_TOOLSTATES_JSON+x}" ]]; then
    cp /tmp/toolstate/toolstates.json "${upload_dir}/${DEPLOY_TOOLSTATES_JSON}"
fi

echo "Files that will be uploaded:"
ls -lah "${upload_dir}"
echo

deploy_dir="rustc-builds"
if [[ "${DEPLOY_ALT-0}" -eq "1" ]]; then
    deploy_dir="rustc-builds-alt"
fi
deploy_url="s3://${DEPLOY_BUCKET}/${deploy_dir}/$(ciCommit)"

retry aws s3 cp --no-progress --recursive --acl public-read "${upload_dir}" "${deploy_url}"
