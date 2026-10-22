//! The guild file's reread keeps the forums the bot made (theseus-p5g63).

use super::*;

/// A guild watched from a file is read again at every request, and the
/// forum the bot made in it stays (theseus-p5g63): it was dropped, so a
/// binding that recorded it found it gone, made a second, and read tags
/// from a channel the stand-in did not know.
#[test]
fn a_forum_the_bot_made_survives_the_guild_files_reread() {
    let fake = FakeDiscord::start();
    let d = tempfile::tempdir().unwrap();
    let file = d.path().join("guild.json");
    std::fs::write(&file, serde_json::to_string(&guild()).unwrap()).unwrap();
    fake.watch_guild_file(&file).unwrap();
    let call = |m: &str, p: &str, b: &Value| http(&fake.addr, m, &format!("/api/v10{p}"), b);
    let body = json!({"name": "theseus", "type": 15, "available_tags": [{"name": "done"}]});
    let made = call("POST", &format!("/guilds/{DEFAULT_GUILD}/channels"), &body).1;
    let id = made["id"].as_str().unwrap().to_string();
    let (status, forum) = call("GET", &format!("/channels/{id}"), &Value::Null);
    assert_eq!((status, forum["type"].as_u64()), (200, Some(15)), "{forum}");
    assert_eq!(forum["available_tags"].as_array().map(Vec::len), Some(1));
    let listed = call(
        "GET",
        &format!("/guilds/{DEFAULT_GUILD}/channels"),
        &Value::Null,
    )
    .1;
    assert!(
        listed.as_array().unwrap().iter().any(|c| c["id"] == id),
        "{listed}"
    );
}
