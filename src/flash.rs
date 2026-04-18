/// Flash music helper: map a mood tag to an ambient track URL.
pub fn music_url_for_mood(mood: &str) -> String {
    let key = mood.trim().to_lowercase();
    match key.as_str() {
        "calm" | "peaceful" => {
            "https://cdn.pixabay.com/download/audio/2022/05/27/audio_1808fbf07a.mp3".into()
        }
        "hopeful" | "uplifting" => {
            "https://cdn.pixabay.com/download/audio/2022/03/15/audio_c91e1e0820.mp3".into()
        }
        "tense" | "anxious" => {
            "https://cdn.pixabay.com/download/audio/2022/10/30/audio_f2c0e18c58.mp3".into()
        }
        "reflective" | "melancholy" => {
            "https://cdn.pixabay.com/download/audio/2021/11/25/audio_00fa5593f3.mp3".into()
        }
        _ => "https://cdn.pixabay.com/download/audio/2022/05/27/audio_1808fbf07a.mp3".into(),
    }
}
