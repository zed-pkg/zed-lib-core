# zed-orm-core

This directory is the opaque SeaORM data-plane package for the Zed registry.
It publishes independently as `zed-pkg/zed-orm-core` and depends on
`zed-pkg/zed-interfaces` through Zed. Default features expose read operations;
writes and migrations require separate opt-in features and database roles.

The authored SQL remains co-located under `sql/`, but declarative-migrations
installs the narrower, dependency-free `zed-pkg/zed-schema` package instead of
this Rust implementation package.
