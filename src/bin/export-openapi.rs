fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("{}", subscription::routes::openapi_document().to_json()?);
    Ok(())
}
