use bee::Client;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let url = std::env::var("BEE_URL").unwrap_or_else(|_| "http://localhost:1633".into());
    let token = std::env::var("BEE_TOKEN").ok();
    let client = match token {
        Some(t) => Client::with_token(&url, &t)?,
        None => Client::new(&url)?,
    };

    println!("=== chequebook_address ===");
    let addr = client.debug().chequebook_address().await?;
    println!("contract: {addr}");

    println!("\n=== check_pins (full sweep) ===");
    let pins = client.api().check_pins(None).await?;
    for p in &pins {
        println!(
            "  {}  total={} missing={} invalid={} {}",
            p.reference.to_hex(),
            p.total,
            p.missing,
            p.invalid,
            if p.is_healthy() { "OK" } else { "BAD" },
        );
    }
    println!("{} pins checked", pins.len());

    Ok(())
}
