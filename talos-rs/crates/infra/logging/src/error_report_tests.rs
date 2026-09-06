use super::*;

#[derive(Debug)]
struct CyclicSource;

impl fmt::Display for CyclicSource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("cyclic source")
    }
}

impl Error for CyclicSource {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(self)
    }
}

#[test]
fn cyclic_dependency_sources_are_explicitly_marked_instead_of_hanging_reporting() {
    let error = TalosError::internal_with_source("read dependency", CyclicSource);
    assert_eq!(
        ErrorSources(&error).to_string(),
        "0: cyclic source\n[source cycle omitted]"
    );
}
