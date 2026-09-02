fastlane documentation
----

# Installation

Make sure you have the latest version of the Xcode command line tools installed:

```sh
xcode-select --install
```

For _fastlane_ installation instructions, see [Installing _fastlane_](https://docs.fastlane.tools/#installing-fastlane)

# Available Actions

## iOS

### ios dry_run

```sh
[bundle exec] fastlane ios dry_run
```

Dry-run: validate lane wiring without Xcode / gym / match

### ios beta

```sh
[bundle exec] fastlane ios beta
```

Build signed IPA via Flutter + gym (macOS + match certs required)

### ios release

```sh
[bundle exec] fastlane ios release
```

App Store release build (macOS). Does not upload.

----

This README.md is auto-generated and will be re-generated every time [_fastlane_](https://fastlane.tools) is run.

More information about _fastlane_ can be found on [fastlane.tools](https://fastlane.tools).

The documentation of _fastlane_ can be found on [docs.fastlane.tools](https://docs.fastlane.tools).
