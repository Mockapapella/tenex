//! Integration coverage for update helpers compiled without `cfg(test)`.

use semver::Version;

#[test]
fn test_update_current_version_matches_cargo_pkg_version() -> Result<(), Box<dyn std::error::Error>>
{
    let expected = Version::parse(env!("CARGO_PKG_VERSION"))?;
    let actual = tenex::update::current_version()?;
    assert_eq!(actual, expected);
    Ok(())
}
