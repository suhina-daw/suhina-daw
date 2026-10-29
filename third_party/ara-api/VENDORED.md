# ARA API headers

The C headers of the ARA (Audio Random Access) API, from
[Celemony/ARA_API](https://github.com/Celemony/ARA_API) at commit
`65ec5c43b943a48cb5446f448a0492db6af8534b` (ARA SDK 2.3), unchanged: `ARAInterface.h`,
`ARAVST3.h`, `ARACLAP.h`, `ChangeLog.txt` and `LICENSE.txt`.

Licence: Apache License 2.0 (`LICENSE.txt`), compatible with Suhina's GPL-3.0-or-later.
`ARA_API`'s README says that implementing the API in a host or plug-in is normal use of it, not
a derived work.

They are not compiled. `scripts/ara_bindings.sh` turns `ARAInterface.h` into
`crates/plugin-host/src/ara_sys/` with bindgen, once for x86 and once for ARM (the headers
pack structs differently on each); the three COM interfaces of `ARAVST3.h` are
written by hand in `crates/plugin-host/src/ara.rs`. See
[ADR 0028](../../docs/decisions/0028-ara-hosting.md).
