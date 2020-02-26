# ![TockOS](http://www.tockos.org/assets/img/tock.svg "TockOS Logo")

[![Build Status](https://travis-ci.org/tock/tock.svg?branch=master)](https://travis-ci.org/tock/tock)
[![irc](https://img.shields.io/badge/irc-%23tock-lightgrey.svg)](https://kiwiirc.com/client/irc.freenode.net/tock)

Tock is an embedded operating system designed for running multiple concurrent, mutually
distrustful applications on Cortex-M based embedded platforms. Tock's design
centers around protection, both from potentially malicious applications and
from device drivers. Tock uses two mechanisms to protect different components
of the operating system. First, the kernel and device drivers are written in
Rust, a systems programming language that provides compile-time memory safety,
type safety and strict aliasing. Tock uses Rust to protect the kernel (e.g. the
scheduler and hardware abstraction layer) from platform specific device drivers
as well as isolate device drivers from each other. Second, Tock uses memory
protection units to isolate applications from each other and the kernel.


Learn More
----------

How would you like to get started?

### Learn How Tock Works

Tock is documented in the [doc](doc) folder. Read through the guides there to
learn about the overview and design of Tock, its implementation, and much
more.


### Use Tock

Follow our [getting started guide](doc/Getting_Started.md) to set up your
system to compile Tock.

Head to the [hardware page](https://www.tockos.org/hardware/)
to learn about the hardware platforms Tock supports. Also check out the
[workshop-style courses](doc/courses) to get started running apps with TockOS.

Find example applications that run on top of the Tock kernel written in both
[Rust](https://github.com/tock/libtock-rs) and
[C](https://github.com/tock/libtock-c).


### Develop Tock

Read our [getting started guide](doc/Getting_Started.md) to get the correct
version of the Rust compiler, then look through the `/kernel`, `/capsules`,
`/chips`, and `/boards` directories.

We encourage contributions back to Tock and are happy to accept pull requests
for anything from small documentation fixes to whole new platforms.
For details, check out our [Contributing Guide](.github/CONTRIBUTING.md).
To get started, please do not hesitate to submit a PR. We'll happily guide you
through any needed changes.


### Keep Up To Date

Check out the [blog](https://www.tockos.org/blog/) where the **Talking Tock**
post series highlights what's new in Tock. Also, follow
[@talkingtock](https://twitter.com/talkingtock) on Twitter.

You can also browse our
[email group](https://groups.google.com/forum/#!forum/tock-dev) 
and our [Slack](https://join.slack.com/t/tockos/shared_invite/enQtNDE5ODQyNDU4NTE1LTg4YzE1MTkwYzI0YjhjNjA0YWExOGY2ZGYwNjQ2YmFiZjdhOTdlMzY0YTBiYTA2YTRlYzMyZTI1MDdmMTgwMzc) to see
discussions on Tock development.


License
-------

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or
  http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or
  http://opensource.org/licenses/MIT)

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall
be dual licensed as above, without any additional terms or conditions.
