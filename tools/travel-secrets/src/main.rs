//! `travel-secrets` — decrypts the source store with the caller's key and encrypts a whitelist
//! to a travel key. Run it yourself; an agent must not run it against a real keyring.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(travel_secrets::run(&args));
}
