use serde::Deserialize;

#[derive(Deserialize, Debug)]
pub struct User {
	pub id: String,
	pub name: String,
	pub chats: Vec<UserRoom>,
}
#[derive(Deserialize, Debug, Clone)]
pub struct UserRoom {
	pub rating: String,
	pub whitelist: Option<Vec<String>>,
	pub id: String,
	pub name: String,
	pub caption: String,
	pub link: Option<String>,
	pub has_tags: bool,
	pub queue: Option<Vec<String>>,
	pub forward: Option<Vec<String>>,
}

#[derive(Deserialize, Debug)]
pub struct Booru {
	pub name: String,
	pub active: bool,
	pub amount: u64,
	pub rating: String,
	pub button: String,
	pub website: String,
	pub api_key: Option<String>,
	pub blacklist: Vec<String>,
	pub replace: Option<Vec<(String, String)>>,
	pub chats: Vec<BooruRoom>,
}
#[derive(Deserialize, Debug)]
pub struct BooruRoom {
	pub id: String,
	pub tags: Vec<String>,
}

#[derive(Deserialize)]
pub struct GelbooruSource {
	pub post: Option<Vec<BooruPost>>,
}
#[derive(Deserialize, Debug, Clone)]
pub struct BooruPost {
	pub id: u64,
	pub file_url: String,
	pub sample_url: String,
	pub md5: Option<String>,
	pub hash: Option<String>,
	pub tags: String,
	pub rating: String,
}

#[derive(Deserialize, Debug, Clone)]
pub struct DanbooruPost {
	pub id: u64,
	pub rating: String,
	pub md5: String,
	pub tag_string: String,
	pub media_asset: DanbooruMediaAsset,
}
#[derive(Deserialize, Debug, Clone)]
pub struct DanbooruMediaAsset {
	pub variants: Vec<DanbooruVariant>,
}
#[derive(Deserialize, Debug, Clone)]
pub struct DanbooruVariant {
	pub r#type: String,
	pub url: String,
}
