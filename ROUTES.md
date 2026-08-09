# Route scheme

Three hosts, three jobs. `zed-api-server.rs` and `zed-web-server.rs` both
implement against this document; `zed-lib-core` is the shared data plane
underneath them.

| Host | Server | Audience |
| --- | --- | --- |
| `api.zpkg.net` | `zed-api-server.rs` | Machines: the `zed` CLI, CI, SDKs |
| `app.zpkg.net` | `zed-web-server.rs` | People: the account console |
| `zpkg.net` | `zed-pkg.github.io` | Marketing (Astro, Pages) |

## The API: registry is a subset, not the root

The mistake to avoid is treating the registry protocol *as* the API. The
registry is the narrow, cacheable, machine-facing slice the package manager
speaks; the account console needs orgs, projects, members, invitations, tokens,
and settings, none of which belong in a package-resolution protocol.

So everything lives under `/v1`, and the registry sits one level down inside it:

```
/v1
├── /registry/…      ← the package-manager protocol. The zed CLI speaks only this.
├── /orgs/…          ← account console
├── /projects/…
├── /users/…
├── /search
├── /tokens/…
└── /health, /ready
```

That split buys three things: `/v1/registry/*` can be cached, rate-limited, and
served anonymously as one coherent group; the CLI's blast radius is one
subtree; and a second protocol version (`/v2/registry`) can ship without
disturbing the console.

### `/v1/registry` — the protocol surface

Packages are addressed the way their URL always has been, `{org}/{name}`.
A project is an org-internal grouping and deliberately does **not** appear in
the path — package names are unique per org (`zed_packages_org_name_active_uq`),
so adding projects broke no URLs.

| Method | Path | Notes |
| --- | --- | --- |
| `GET` | `/v1/registry/packages/{org}/{name}` | Metadata + latest version |
| `GET` | `/v1/registry/packages/{org}/{name}/versions` | Version list, yanked flagged not hidden |
| `GET` | `/v1/registry/packages/{org}/{name}/versions/{version}` | One version, with its license |
| `GET` | `/v1/registry/packages/{org}/{name}/versions/{version}/download` | `302` to a short-lived signed R2 URL |
| `PUT` | `/v1/registry/packages/{org}/{name}/versions/{version}` | Publish. Bearer token, `publish` role |
| `POST` | `/v1/registry/packages/{org}/{name}/versions/{version}/yank` | Yank / unyank |
| `GET` | `/v1/registry/index/{prefix}/{org}/{name}` | Sparse index for resolution |
| `GET` | `/v1/registry/search?q=` | Anonymous search over public packages |

**Downloads.** The server never proxies artifact bytes. It authorizes, records
the download, and redirects to a signed R2 URL, so bandwidth stays on
Cloudflare. The ledger row is written *before* the redirect — a download that
was authorized counts, because that count feeds the promotion rule.

**Uploads.** `PUT` creates a `zed_package_uploads` row in `pending`, streams the
body to R2 under `{org}/{name}/{version}/{sha256}.{format}`, verifies the digest,
and only then creates the `zed_package_versions` row and marks the upload
`verified`. Failed attempts stay in the table; they are the forensic record.

### `/v1/orgs`, `/v1/projects`, `/v1/users` — the console surface

Session-authenticated (shared-auth cookie), not token-authenticated.

```
GET    /v1/orgs                                     orgs for the session user
POST   /v1/orgs                                     create org (creator becomes owner)
GET    /v1/orgs/{org}
PATCH  /v1/orgs/{org}                               settings
GET    /v1/orgs/{org}/members
POST   /v1/orgs/{org}/invitations                   invite
POST   /v1/orgs/{org}/invitations/{token}/accept
GET    /v1/orgs/{org}/projects
POST   /v1/orgs/{org}/projects                      create project
GET    /v1/orgs/{org}/packages
POST   /v1/orgs/{org}/packages                      create package (starts private)
PATCH  /v1/orgs/{org}/packages/{name}               settings
PUT    /v1/orgs/{org}/packages/{name}/visibility    the 10-day / 50-download rule
GET    /v1/orgs/{org}/packages/{name}/archive       download as zip or tarball
GET    /v1/projects/{id}  /  PATCH  /  members  /  invitations
GET    /v1/users/me       /  PATCH  /v1/users/me    user settings
GET    /v1/tokens         /  POST   /  DELETE       publish tokens
```

Package *management* is `/v1/orgs/{org}/packages/{name}` while package
*consumption* is `/v1/registry/packages/{org}/{name}`. Same row, two audiences,
two authorization models — worth the apparent duplication, because collapsing
them would put console-only mutations inside the cacheable protocol subtree.

`PUT /visibility` is the one route that can return `409`. `zed-lib-core` maps
SQLSTATE `ZD001`/`ZD002` to typed errors, and the handler renders whichever
refusal applies rather than a generic failure.

## The web console: `app.zpkg.net`

Server-rendered Maud, HTMX for partial updates, no client framework.

| Path | Page |
| --- | --- |
| `/` | Home — the user's orgs, plus search across projects and packages |
| `/dashboard/{org}` | Per-org dashboard: its projects and packages |
| `/orgs/{org}/settings` | Invite members, create project/package/org |
| `/orgs/{org}/projects/{project}/settings` | Invite members, add packages |
| `/orgs/{org}/packages/{name}/settings` | Configure; download as zip or tarball |
| `/settings` | Settings for the signed-in user |
| `/p/{org}/{name}` | Public package page |
| `/search` | Search results |
| `/partials/*` | HTMX fragments — never a full layout |

Signed out, `/` and `/p/{org}/{name}` show public packages only. The header is
static in structure and context-aware in content: it always renders the same
skeleton, and fills the org/project/package switchers plus the "create" dropdown
from the current route and session.

The web tier holds a **SELECT-only** database identity and calls the API over
HTTP keep-alive for every mutation. It has no write credential to fall back on,
which is why `zed-lib-core` refuses to hand it a `WriteContext` at compile time.
