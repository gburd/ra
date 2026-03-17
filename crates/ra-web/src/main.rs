//! Web explorer backend API.

use rocket::{get, launch, routes};

#[get("/")]
fn index() -> &'static str {
    "Relational Algebra Web Explorer API"
}

#[get("/health")]
fn health() -> &'static str {
    "OK"
}

#[launch]
fn rocket() -> _ {
    rocket::build().mount("/", routes![index, health])
}
