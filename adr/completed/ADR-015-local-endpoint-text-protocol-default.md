# ADR-015: Local Endpoint Text-Protocol Default

**Status:** Accepted  
**See also:** ADR-003, ADR-047

## Decision

- Local endpoints include loopback, RFC 1918 private IPv4, IPv4 link-local (`169.254.0.0/16`), IPv6 link-local (`fe80::/10`), and IPv6 unique-local (`fc00::/7`) ranges.
- `is_local_endpoint_url` determines locality; no manual override is required for recognized local addresses.

## References

- [RFC 1918](https://www.rfc-editor.org/rfc/rfc1918) — private address space
- [RFC 3927](https://www.rfc-editor.org/rfc/rfc3927) — IPv4 link-local addresses
- [RFC 4193](https://www.rfc-editor.org/rfc/rfc4193) — IPv6 unique local addresses
- [RFC 4291 §2.5.6](https://www.rfc-editor.org/rfc/rfc4291#section-2.5.6) — IPv6 link-local addresses
