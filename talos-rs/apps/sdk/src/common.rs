use crate::ExpectedFileVersion;

pub(crate) fn expected_file_version_token(expected_version: ExpectedFileVersion) -> Option<String> {
    match expected_version {
        ExpectedFileVersion::MustNotExist => None,
        ExpectedFileVersion::Match { version_token } => Some(version_token),
    }
}
