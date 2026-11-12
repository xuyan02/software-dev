use browser_capsule::BrowserCapsule;
use std::time::Duration;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let output = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "baidu.png".to_string());

    let capsule = BrowserCapsule::create().await?;
    capsule.navigate("https://www.baidu.com").await?;

    // Baidu renders some content after readyState becomes complete.
    tokio::time::sleep(Duration::from_secs(2)).await;

    let png = capsule.screenshot().await?;
    std::fs::write(&output, &png)?;
    println!("saved {output} ({} bytes)", png.len());

    capsule.close().await?;
    Ok(())
}
