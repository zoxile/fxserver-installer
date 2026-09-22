# Contributor Attribution

## Hunter Corlett

The 0.5.0 Beta feature batch includes ideas and selected code adapted from [Hunter Corlett (@Huntercorlett)](https://github.com/Huntercorlett) and the [fxserver-installer-reborn fork](https://github.com/Huntercorlett/fxserver-installer-reborn), revision [`53f183437438109093b4a73a747b1d650ce31dc9`](https://github.com/Huntercorlett/fxserver-installer-reborn/commit/53f183437438109093b4a73a747b1d650ce31dc9).

The selected work covers Enhanced server executable/download support, txData discovery, database administration and inspection, remembered-login UX, MariaDB release/compatibility choices, and reducing repeated database discovery work.

This is a selective adaptation, not a merge of the whole fork. The implementation retains Queries & Files, adds bounded and reviewed operations, and does not adopt automatic SQL repair/retry, a second database driver, website hosting, or enforced credit checks. Protected credential storage and review-only SQL diagnostics were implemented for this app's existing security boundaries.

Relevant commits and release notes also credit the source. Existing project licensing continues to apply; see [LICENSE](../LICENSE).

## Data Providers

- Legacy artifact recommendation and issue metadata: [JG Scripts Artifacts DB](https://artifacts.jgscripts.com/).
- Legacy and Enhanced server binaries: official [Cfx.re server downloads](https://docs.fivem.net/docs/server-download/).
- MariaDB Windows packages and release metadata: [MariaDB Foundation downloads](https://mariadb.org/download/).
