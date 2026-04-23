# ADR-015: Local Endpoint Text-Protocol Default

**Status:** Accepted  
**See also:** ADR-003, ADR-047

## Decision

- Local endpoints (localhost / RFC 1918 ranges) default to text-stream protocol without TLS.
- `is_local_endpoint_url` determines locality; no manual override required for loopback.

## References

- [RFC 1918](https://www.rfc-editor.org/rfc/rfc1918) — private address space
