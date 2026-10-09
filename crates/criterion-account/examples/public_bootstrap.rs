//! One explicitly invoked anonymous request; no subscriber credentials or routes.
use criterion_account::{AccountClient, HttpTransport};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), criterion_account::Error> {
    let account = AccountClient::with_transport(HttpTransport::new()?);
    let region = account.bootstrap().await?;
    println!("Anonymous bootstrap admitted region {region:?}; opaque token retained privately.");
    account.dispose();
    Ok(())
}
