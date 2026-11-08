## Summary

What changed and why?

## Testing

- [ ] `cargo test --workspace --locked`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `make dist` / `lintian` (if packaging changed)
- [ ] Tested on real hardware (if fan-control behavior changed)

## Notes

Anything reviewers should know: hardware tested, migration concerns, protocol
changes (remember to bump `PROTOCOL_VERSION`).
