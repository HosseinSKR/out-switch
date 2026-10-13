# Contributing

Out-Switch 0.4 is a routing test build by HosseinSKR. Read README.md, the architecture
and validation record before describing functionality or making a release.

Use Windows x64, Rust 1.99.0, C++ build tools and Windows SDK. Set
`RUSTFLAGS=-C target-feature=+crt-static` for MSVC, then run `cargo test --workspace --locked`
and `cargo build --workspace --release --locked`. The resource build locates the SDK's
resource compiler; GNU builds can set WINDRES to their resource compiler.

Run `scripts/Get-WinDivert.ps1` before packaging. It supplies the signed driver, DLL,
license and matching source. The old custom WDK project is historical and not used.
No driver-signing bypass is permitted. An access-denied integration test must be recorded
as unverified, not rewritten to pass or worked around by weakening permissions.

Make focused changes. Never log traffic payloads, decrypt TLS, inject code into apps,
add telemetry or alter global routes. Test new routing on a dedicated machine first.
Tests that intercept traffic must be explicit, bounded and restore handles on exit.
Review the full RELEASE.md gates before stable publication. The CI workflow uploads
build artifacts only; publishing requires the owner's explicit approval.
