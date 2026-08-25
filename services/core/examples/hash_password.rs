//! Print Argon2id PHC hash for local seed scripts: `cargo run -p companyos-core --example hash_password -- 'correct-horse-battery'`

fn main() {
    let password = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "correct-horse-battery".into());
    let (hash, salt) =
        companyos_core::auth::password::hash_password(&password).expect("hash password");
    println!("{hash}");
    eprintln!("salt={salt}");
}
