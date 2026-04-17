# Support

Thanks for using Kimberlite. Here's where to turn for help.

## Looking for answers?

- **Docs** — [kimberlite.dev](https://kimberlite.dev) is the fastest path. Start with [Quick Start](https://kimberlite.dev/docs/quick-start), then the [Concepts](https://kimberlite.dev/docs/concepts) and [Operating](https://kimberlite.dev/docs/operating) sections.
- **SQL reference** — [`docs/reference/sql/`](../docs/reference/sql/). If a feature is stubbed we return a typed parse error — search the error message.
- **FAQ** — [`docs/reference/faq.md`](../docs/reference/faq.md).

## Have a question?

Use **[GitHub Discussions](https://github.com/kimberlitedb/kimberlite/discussions)** or the **[Discord community](https://discord.gg/QPChWYjD)**:

- **Q&A** — best-effort community answers, searchable for future users.
- **Ideas** — feature requests and design discussions (before opening an issue).
- **Show and tell** — share what you're building.

Please search existing threads before posting.

## Found a bug?

Open a **[GitHub Issue](https://github.com/kimberlitedb/kimberlite/issues/new/choose)**. A good bug report includes:

- Kimberlite version (`kimberlite --version`) and OS.
- Minimal reproduction — commands, schema, data, expected vs actual output.
- Relevant logs (set `RUST_LOG=kimberlite=debug`).
- Stack trace if the process crashed.

If the bug is in an SDK, include the SDK version and language toolchain version.

## Found a security issue?

**Do not open a public issue.** See [SECURITY.md](../SECURITY.md) for the private disclosure path. Initial response within 48 hours.

## Commercial support

Commercial support is not yet offered. For production deployments, reach out on Discord — we're interested in understanding your needs.

## What we don't use the issue tracker for

- General "how do I do X" questions — use Discussions or Discord.
- "Can you explain how consensus works?" — start with [`docs/internals/`](../docs/internals/).
- Feature requests without a use case — share in Discussions first so we can discuss design trade-offs.

Keeping issues focused on actionable bugs and concrete feature proposals helps us triage and ship faster.
