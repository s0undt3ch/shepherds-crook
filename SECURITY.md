# Security policy

## Reporting a vulnerability

Please report vulnerabilities privately, through GitHub's private vulnerability reporting: open the
repository's **Security** tab and choose **Report a vulnerability**. Do not open a public issue or pull
request for a vulnerability.

Include what you found, how to reproduce it, and the crook version (`crook --version`) and platform. You
will get a reply in the advisory thread, and a fix is credited to you unless you would rather it were not.

## Scope

In scope:

- the `crook` binary and its Claude Code mod, as shipped from this repository;
- the privacy boundary: any way a session can send a private note path, or a private location's
  contents, out through a tool call without the hook asking first, beyond the README's "Known gaps".

Out of scope: Claude Code itself, herdr, and other tools crook starts or talks to. Report those to
their own maintainers.

## Bounty

There is no bug bounty.
