# Repository instructions

## Closure: install the binary and clean up

When a change is complete, close it out in this order:

1. Run `cargo fmt --check`, `cargo clippy --all-targets` and `cargo test`. Fix failures before going on.
2. Commit the change locally.
3. Build the release binary and replace the installed one atomically:

   ```sh
   cargo build --release
   install -m 755 target/release/sizr ~/.local/bin/.sizr.new
   mv -f ~/.local/bin/.sizr.new ~/.local/bin/sizr
   ```

   `~/.local/bin` comes first on Stefan's `PATH`. Never overwrite the installed file in place (for example `cp` onto it): macOS caches code signatures and can kill the rewritten binary.
4. Verify: `cmp target/release/sizr ~/.local/bin/sizr` succeeds, `command -v sizr` prints `~/.local/bin/sizr`, and `sizr --version` runs.
5. Remove the build output with `cargo clean`. Keep `Cargo.lock`, source, configuration and any user data.

Installing the binary does not need a version bump.

## Git workflow

- Commit locally as work progresses. Push only when Stefan asks; when a milestone is done, ask once whether to push now or not yet.
- Use plain `git` on `main`. Create branches or pull requests only when asked.
