//! Print the Parser-owned Hub upgrade profile for deployment or regeneration.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    print!("{}", talos_sync_parser::v1_upgrade_mapping_json()?);
    Ok(())
}
