//! `signature::public_key()` reads `CHAOS_SIGNING_PUBLIC_KEY` through `option_env!`,
//! which is resolved by the compiler. Cargo tracks no environment variables by
//! default, so without this directive a rebuild after changing the key relinks
//! nothing and silently keeps whichever key the previous build happened to embed.
//!
//! That is the failure mode this guards: an operator builds with the release key,
//! then builds again without it (or the other way round) and gets a binary whose
//! embedded key does not match the command they just ran.

fn main() {
    println!("cargo:rerun-if-env-changed=CHAOS_SIGNING_PUBLIC_KEY");
}
