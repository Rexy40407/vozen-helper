fn main() {
    let output = std::env::args().nth(1).expect("output PNG path required");
    let card = helper_discord::starboard_card::Card {
        author: "Rexy",
        message: "batata",
        channel: "aaaaa",
        stars: 2,
        avatar: None,
    };
    std::fs::write(
        output,
        helper_discord::starboard_card::render(&card).expect("PNG"),
    )
    .expect("write preview");
}
