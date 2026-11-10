# time-zone-offset-calculator

Applies POSIX `TZ` environment variable rules to compute UTC offsets for a given local instant.

```rust
use time_zone_offset_calculator::{LocalDateTime, TzSpec};

let tz = TzSpec::parse("EST5EDT,M3.2.0,M11.1.0").unwrap();

// Winter: standard time, UTC-05:00.
let winter = LocalDateTime::new(2024, 1, 15, 12, 0, 0);
assert_eq!(tz.offset_for_local(winter), -5 * 3600);

// Summer: daylight time, UTC-04:00.
let summer = LocalDateTime::new(2024, 7, 1, 12, 0, 0);
assert_eq!(tz.offset_for_local(summer), -4 * 3600);
```

## Why this exists

Most timezone libraries ship a compiled copy of the IANA database. That is the right
choice when you need historical accuracy, but it is a large dependency and it is stale
the moment a government changes its rules. This crate deliberately does less: it parses
the string that a POSIX system would put in `TZ` and evaluates it directly. The
trade-off is that it knows nothing about history or politics — it applies whatever rule
you hand it, for any year, extrapolated indefinitely. If you need the 1974 rules for
somewhere, this is not the crate for you.

## The awkward edge

Twice a year the mapping from local wall time to UTC is not a function.

During the spring-forward transition a range of local times does not exist. This crate
resolves that gap by returning the standard-time offset, i.e. the offset in effect just
before the transition.

During the fall-back transition a range of local times occurs twice. This crate returns
the **earlier** occurrence, i.e. the daylight-saving offset.

Both choices are deliberate and are covered by tests. If you need the other
interpretation you will have to disambiguate before calling `offset_for_local`.

## API

- `TzSpec::parse(&str) -> Result<TzSpec, TzError>` — parse a POSIX TZ string. A leading
  `:` is accepted and ignored.
- `TzSpec::offset_for_local(LocalDateTime) -> i64` — offset in seconds east of UTC for a
  local wall time.
- `TzSpec::offset_for_utc(i64) -> i64` — offset in seconds east of UTC for a UTC instant
  expressed as Unix seconds.
- `TzSpec::has_dst() -> bool` — whether the spec carries a daylight-saving component.
- `LocalDateTime::new(year, month, day, hour, minute, second)` — construct a civil time.
- `LocalDateTime::to_unix_seconds()` / `LocalDateTime::from_unix_seconds(i64)` — convert
  between civil time treated as UTC and Unix seconds.
- `TzError` — parse failures: `Empty`, `BadNumber`, `BadRule`, `UnterminatedQuote`,
  `OffsetOutOfRange`.

The public fields of `TzSpec` (`std_abbr`, `std_offset`, `dst_abbr`, `dst_offset`,
`dst_rule`) and of `DstRule` / `TransitionRule` are all part of the API.

## Scope

Only the POSIX grammar is supported: `std offset [dst [offset] [,rule]]`, where a rule
is `,Mm.w.d[/time],Mm.w.d[/time]`. The `right/` prefix and file-backed zone names are not
supported. Offsets are limited to 24 hours in magnitude.
