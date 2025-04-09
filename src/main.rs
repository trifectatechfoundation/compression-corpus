mod gz_header;

fn main() -> std::io::Result<()> {
    let gz_header = gz_header::gz_header();
    std::fs::write("handpicked/gz_header.gz", &gz_header)?;

    Ok(())
}
