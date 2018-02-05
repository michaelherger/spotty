use std::env;
use std::fs::File;
use std::io::Write;
use std::path::Path;

fn main() {
    // create empty client_id.txt if it doesn't exist yet
    let clientid_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let clientid_path = Path::new(&clientid_dir).join("src").join("client_id.txt");

    if ! clientid_path.exists() {
      let mut f = File::create(&clientid_path).unwrap();
      f.write_all("".as_bytes()).unwrap();
    }
}