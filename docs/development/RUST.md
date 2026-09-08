# Rust implementation conventions

Use for Rust implementation and test changes. Commands and delivery gates are owned by [Development workflow](DEVELOPMENT.md#scoped-validation).

## Code and API shape

- Use the `talos-*` crate naming convention and inherit workspace lints. Prefer private modules with explicit public exports; do not expose test-only APIs.
- Follow `uninlined_format_args`, `collapsible_if`, and `redundant_closure_for_method_calls`: use `format!("{name}")`, collapse eligible conditions, and prefer method references over redundant closures.
- Avoid opaque positional `bool` or `Option` arguments. Prefer enums, named methods, newtypes, builders, or parameter objects. When the API cannot change, use precise argument comments such as `foo(/*enabled*/ false)` or `bar(/*timeout*/ None)`. A sole argument matching the method name, such as `.enabled(false)`, needs no comment; do not mechanically annotate string or character literals.
- Prefer exhaustive enum matches over wildcard arms that hide new variants.
- Prefer native RPITIT for async traits with explicit `Send` futures: `fn foo(&self) -> impl std::future::Future<Output = T> + Send;`. Implementations may use `async fn` when they satisfy that contract. Avoid `#[async_trait]` and do not use `#[allow(async_fn_in_trait)]` to omit the future contract.
- A single-use helper is justified only when it meaningfully expresses a concept or reduces complexity.
- Prefer function-level `#[tracing::instrument(...)]` over call-site future instrumentation. Check the callee or delegated implementation first to avoid duplicate spans.

## Test organization

- Prefer complete-object equality and the existing `pretty_assertions::assert_eq` convention.
- Put unit tests in sibling `*_tests.rs` files using `#[path = "..._tests.rs"] mod tests;`. Resolve workspace test binary paths across build systems and working directories, not incidental shell-relative paths.
