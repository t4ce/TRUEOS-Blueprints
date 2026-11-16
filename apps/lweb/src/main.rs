// trueos-blueprint: features=["tokio-net-probe"]

fn main() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
    let runtime = trueos::runtime::current_thread_net().build()?;
    #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 8787)).await?;
        println!("lweb: http://127.0.0.1:8787/");
        axum::serve(listener, lweb::router()).await
    })?;
    Ok(())
}
