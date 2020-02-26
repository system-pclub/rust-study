#!/usr/bin/env python
# -*- coding: utf-8 -*-

## This script publishes the new "current" toolstate in the toolstate repo (not to be
## confused with publishing the test results, which happens in
## `src/ci/docker/x86_64-gnu-tools/checktools.sh`).
## It is set as callback for `src/ci/docker/x86_64-gnu-tools/repo.sh` by the CI scripts
## when a new commit lands on `master` (i.e., after it passed all checks on `auto`).

from __future__ import print_function

import sys
import re
import os
import json
import datetime
import collections
import textwrap
try:
    import urllib2
except ImportError:
    import urllib.request as urllib2

# List of people to ping when the status of a tool or a book changed.
# These should be collaborators of the rust-lang/rust repository (with at least
# read privileges on it). CI will fail otherwise.
MAINTAINERS = {
    'miri': {'oli-obk', 'RalfJung', 'eddyb'},
    'clippy-driver': {
        'Manishearth', 'llogiq', 'mcarton', 'oli-obk', 'phansch', 'flip1995',
        'yaahc',
    },
    'rls': {'Xanewok'},
    'rustfmt': {'topecongiro'},
    'book': {'carols10cents', 'steveklabnik'},
    'nomicon': {'frewsxcv', 'Gankra'},
    'reference': {'steveklabnik', 'Havvy', 'matthewjasper', 'ehuss'},
    'rust-by-example': {'steveklabnik', 'marioidival'},
    'embedded-book': {
        'adamgreig', 'andre-richter', 'jamesmunns', 'korken89',
        'ryankurte', 'thejpster', 'therealprof',
    },
    'edition-guide': {'ehuss', 'Centril', 'steveklabnik'},
    'rustc-guide': {'mark-i-m', 'spastorino', 'amanjeev', 'JohnTitor'},
}

REPOS = {
    'miri': 'https://github.com/rust-lang/miri',
    'clippy-driver': 'https://github.com/rust-lang/rust-clippy',
    'rls': 'https://github.com/rust-lang/rls',
    'rustfmt': 'https://github.com/rust-lang/rustfmt',
    'book': 'https://github.com/rust-lang/book',
    'nomicon': 'https://github.com/rust-lang/nomicon',
    'reference': 'https://github.com/rust-lang/reference',
    'rust-by-example': 'https://github.com/rust-lang/rust-by-example',
    'embedded-book': 'https://github.com/rust-embedded/book',
    'edition-guide': 'https://github.com/rust-lang/edition-guide',
    'rustc-guide': 'https://github.com/rust-lang/rustc-guide',
}


def validate_maintainers(repo, github_token):
    '''Ensure all maintainers are assignable on a GitHub repo'''
    next_link_re = re.compile(r'<([^>]+)>; rel="next"')

    # Load the list of assignable people in the GitHub repo
    assignable = []
    url = 'https://api.github.com/repos/%s/collaborators?per_page=100' % repo
    while url is not None:
        response = urllib2.urlopen(urllib2.Request(url, headers={
            'Authorization': 'token ' + github_token,
            # Properly load nested teams.
            'Accept': 'application/vnd.github.hellcat-preview+json',
        }))
        assignable.extend(user['login'] for user in json.load(response))
        # Load the next page if available
        url = None
        link_header = response.headers.get('Link')
        if link_header:
            matches = next_link_re.match(link_header)
            if matches is not None:
                url = matches.group(1)

    errors = False
    for tool, maintainers in MAINTAINERS.items():
        for maintainer in maintainers:
            if maintainer not in assignable:
                errors = True
                print(
                    "error: %s maintainer @%s is not assignable in the %s repo"
                    % (tool, maintainer, repo),
                )

    if errors:
        print()
        print("  To be assignable, a person needs to be explicitly listed as a")
        print("  collaborator in the repository settings. The simple way to")
        print("  fix this is to ask someone with 'admin' privileges on the repo")
        print("  to add the person or whole team as a collaborator with 'read'")
        print("  privileges. Those privileges don't grant any extra permissions")
        print("  so it's safe to apply them.")
        print()
        print("The build will fail due to this.")
        exit(1)

def read_current_status(current_commit, path):
    '''Reads build status of `current_commit` from content of `history/*.tsv`
    '''
    with open(path, 'rU') as f:
        for line in f:
            (commit, status) = line.split('\t', 1)
            if commit == current_commit:
                return json.loads(status)
    return {}

def gh_url():
    return os.environ['TOOLSTATE_ISSUES_API_URL']

def maybe_delink(message):
    if os.environ.get('TOOLSTATE_SKIP_MENTIONS') is not None:
        return message.replace("@", "")
    return message

def issue(
    tool,
    status,
    assignees,
    relevant_pr_number,
    relevant_pr_user,
    pr_reviewer,
):
    # Open an issue about the toolstate failure.
    if status == 'test-fail':
        status_description = 'has failing tests'
    else:
        status_description = 'no longer builds'
    request = json.dumps({
        'body': maybe_delink(textwrap.dedent('''\
        Hello, this is your friendly neighborhood mergebot.
        After merging PR {}, I observed that the tool {} {}.
        A follow-up PR to the repository {} is needed to fix the fallout.

        cc @{}, do you think you would have time to do the follow-up work?
        If so, that would be great!

        cc @{}, the PR reviewer, and nominating for compiler team prioritization.

        ''').format(
            relevant_pr_number, tool, status_description,
            REPOS.get(tool), relevant_pr_user, pr_reviewer
        )),
        'title': '`{}` no longer builds after {}'.format(tool, relevant_pr_number),
        'assignees': list(assignees),
        'labels': ['T-compiler', 'I-nominated'],
    })
    print("Creating issue:\n{}".format(request))
    response = urllib2.urlopen(urllib2.Request(
        gh_url(),
        request,
        {
            'Authorization': 'token ' + github_token,
            'Content-Type': 'application/json',
        }
    ))
    response.read()

def update_latest(
    current_commit,
    relevant_pr_number,
    relevant_pr_url,
    relevant_pr_user,
    pr_reviewer,
    current_datetime
):
    '''Updates `_data/latest.json` to match build result of the given commit.
    '''
    with open('_data/latest.json', 'rb+') as f:
        latest = json.load(f, object_pairs_hook=collections.OrderedDict)

        current_status = {
            os: read_current_status(current_commit, 'history/' + os + '.tsv')
            for os in ['windows', 'linux']
        }

        slug = 'rust-lang/rust'
        message = textwrap.dedent('''\
            📣 Toolstate changed by {}!

            Tested on commit {}@{}.
            Direct link to PR: <{}>

        ''').format(relevant_pr_number, slug, current_commit, relevant_pr_url)
        anything_changed = False
        for status in latest:
            tool = status['tool']
            changed = False
            create_issue_for_status = None # set to the status that caused the issue

            for os, s in current_status.items():
                old = status[os]
                new = s.get(tool, old)
                status[os] = new
                maintainers = ' '.join('@'+name for name in MAINTAINERS[tool])
                # comparing the strings, but they are ordered appropriately:
                # "test-pass" > "test-fail" > "build-fail"
                if new > old:
                    # things got fixed or at least the status quo improved
                    changed = True
                    message += '🎉 {} on {}: {} → {} (cc {}, @rust-lang/infra).\n' \
                        .format(tool, os, old, new, maintainers)
                elif new < old:
                    # tests or builds are failing and were not failing before
                    changed = True
                    title = '💔 {} on {}: {} → {}' \
                        .format(tool, os, old, new)
                    message += '{} (cc {}, @rust-lang/infra).\n' \
                        .format(title, maintainers)
                    # See if we need to create an issue.
                    if tool == 'miri':
                        # Create issue if tests used to pass before. Don't open a *second*
                        # issue when we regress from "test-fail" to "build-fail".
                        if old == 'test-pass':
                            create_issue_for_status = new
                    else:
                        # Create issue if things no longer build.
                        # (No issue for mere test failures to avoid spurious issues.)
                        if new == 'build-fail':
                            create_issue_for_status = new

            if create_issue_for_status is not None:
                try:
                    issue(
                        tool, create_issue_for_status, MAINTAINERS.get(tool, ''),
                        relevant_pr_number, relevant_pr_user, pr_reviewer,
                    )
                except urllib2.HTTPError as e:
                    # network errors will simply end up not creating an issue, but that's better
                    # than failing the entire build job
                    print("HTTPError when creating issue for status regression: {0}\n{1}"
                          .format(e, e.read()))
                except IOError as e:
                    print("I/O error when creating issue for status regression: {0}".format(e))
                except:
                    print("Unexpected error when creating issue for status regression: {0}"
                          .format(sys.exc_info()[0]))
                    raise

            if changed:
                status['commit'] = current_commit
                status['datetime'] = current_datetime
                anything_changed = True

        if not anything_changed:
            return ''

        f.seek(0)
        f.truncate(0)
        json.dump(latest, f, indent=4, separators=(',', ': '))
        return message


if __name__ == '__main__':
    repo = os.environ.get('TOOLSTATE_VALIDATE_MAINTAINERS_REPO')
    if repo:
        github_token = os.environ.get('TOOLSTATE_REPO_ACCESS_TOKEN')
        if github_token:
            validate_maintainers(repo, github_token)
        else:
            print('skipping toolstate maintainers validation since no GitHub token is present')
        # When validating maintainers don't run the full script.
        exit(0)

    cur_commit = sys.argv[1]
    cur_datetime = datetime.datetime.utcnow().strftime('%Y-%m-%dT%H:%M:%SZ')
    cur_commit_msg = sys.argv[2]
    save_message_to_path = sys.argv[3]
    github_token = sys.argv[4]

    # assume that PR authors are also owners of the repo where the branch lives
    relevant_pr_match = re.search(
        r'Auto merge of #([0-9]+) - ([^:]+):[^,]+, r=(\S+)',
        cur_commit_msg,
    )
    if relevant_pr_match:
        number = relevant_pr_match.group(1)
        relevant_pr_user = relevant_pr_match.group(2)
        relevant_pr_number = 'rust-lang/rust#' + number
        relevant_pr_url = 'https://github.com/rust-lang/rust/pull/' + number
        pr_reviewer = relevant_pr_match.group(3)
    else:
        number = '-1'
        relevant_pr_user = 'ghost'
        relevant_pr_number = '<unknown PR>'
        relevant_pr_url = '<unknown>'
        pr_reviewer = 'ghost'

    message = update_latest(
        cur_commit,
        relevant_pr_number,
        relevant_pr_url,
        relevant_pr_user,
        pr_reviewer,
        cur_datetime
    )
    if not message:
        print('<Nothing changed>')
        sys.exit(0)

    print(message)

    if not github_token:
        print('Dry run only, not committing anything')
        sys.exit(0)

    with open(save_message_to_path, 'w') as f:
        f.write(message)

    # Write the toolstate comment on the PR as well.
    issue_url = gh_url() + '/{}/comments'.format(number)
    response = urllib2.urlopen(urllib2.Request(
        issue_url,
        json.dumps({'body': maybe_delink(message)}),
        {
            'Authorization': 'token ' + github_token,
            'Content-Type': 'application/json',
        }
    ))
    response.read()
