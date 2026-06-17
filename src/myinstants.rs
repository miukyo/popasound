use rocket::http::Status;
use rocket::serde::json::Json;
use rocket::serde::Serialize;
use rocket::get;
use regex::Regex;
use scraper::{Html, Selector};

const BASE_URL: &str = "https://www.myinstants.com";
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

#[derive(Serialize)]
pub struct ApiResponse<T: Serialize> {
    status: String,
    author: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

#[derive(Serialize)]
pub struct Sound {
    id: String,
    title: String,
    url: String,
    mp3: String,
}

#[derive(Serialize)]
pub struct SoundDetail {
    id: String,
    url: String,
    title: String,
    mp3: String,
    description: String,
    tags: Vec<String>,
    favorites: String,
    views: String,
    uploader: Uploader,
}

#[derive(Serialize)]
pub struct Uploader {
    username: String,
    url: String,
}

async fn fetch_html(url: &str) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client.get(url).send().await.map_err(|e| e.to_string())?;
    let status = resp.status();
    let body = resp.text().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!("Fetch failed: HTTP {}", status));
    }
    Ok(body)
}

fn parse_sounds(html: &Html) -> Vec<Sound> {
    let play_re = Regex::new(r#"play\('(.*?)'"#).unwrap();
    let instant_sel = Selector::parse("div.instant").unwrap();
    let link_sel = Selector::parse("a.instant-link").unwrap();
    let btn_sel = Selector::parse("button.small-button").unwrap();

    let mut sounds = Vec::new();
    for instant in html.select(&instant_sel) {
        let link = match instant.select(&link_sel).next() {
            Some(l) => l,
            None => continue,
        };
        let title = link.text().collect::<String>().trim().to_string();
        let href = link.value().attr("href").unwrap_or("");
        let id = href.trim_start_matches("/en/instant/").trim_end_matches('/');
        let url = format!("{}{}", BASE_URL, href);

        let btn = match instant.select(&btn_sel).next() {
            Some(b) => b,
            None => continue,
        };
        let onclick = btn.value().attr("onclick").unwrap_or("");
        let mp3 = play_re
            .captures(onclick)
            .and_then(|c| c.get(1))
            .map(|m| format!("{}{}", BASE_URL, m.as_str()))
            .unwrap_or_default();

        sounds.push(Sound {
            id: id.to_string(),
            title,
            url,
            mp3,
        });
    }
    sounds
}

fn ok_response<T: Serialize>(data: T) -> Json<ApiResponse<T>> {
    Json(ApiResponse {
        status: "200".to_string(),
        author: "abdipr".to_string(),
        data: Some(data),
        message: None,
    })
}

fn err_response(status: u16, msg: &str) -> (Status, Json<ApiResponse<()>>) {
    (
        Status::from_code(status).unwrap_or(Status::NotFound),
        Json(ApiResponse {
            status: status.to_string(),
            author: "abdipr".to_string(),
            data: None,
            message: Some(msg.to_string()),
        }),
    )
}

#[get("/")]
pub fn index() -> Json<ApiResponse<()>> {
    Json(ApiResponse {
        status: "200".to_string(),
        author: "abdipr".to_string(),
        data: None,
        message: Some("Check https://github.com/abdipr/myinstants-api for documentation".to_string()),
    })
}

#[get("/recent")]
pub async fn recent() -> Result<Json<ApiResponse<Vec<Sound>>>, (Status, Json<ApiResponse<()>>)> {
    let body = fetch_html("https://www.myinstants.com/en/recent").await.map_err(|e| err_response(404, &e))?;
    let html = Html::parse_document(&body);
    Ok(ok_response(parse_sounds(&html)))
}

#[get("/trending?<q>")]
pub async fn trending(q: String) -> Result<Json<ApiResponse<Vec<Sound>>>, (Status, Json<ApiResponse<()>>)> {
    if q.is_empty() {
        return Err(err_response(404, "Query parameter 'q' is required, example: ?q=id"));
    }
    let url = format!("https://www.myinstants.com/en/index/{}", q);
    let body = fetch_html(&url).await.map_err(|e| err_response(404, &e))?;
    let html = Html::parse_document(&body);
    Ok(ok_response(parse_sounds(&html)))
}

#[get("/search?<q>")]
pub async fn search(q: String) -> Result<Json<ApiResponse<Vec<Sound>>>, (Status, Json<ApiResponse<()>>)> {
    if q.is_empty() {
        return Err(err_response(404, "Query parameter 'q' is required, example: ?q=vine boom"));
    }
    let url = format!("https://www.myinstants.com/en/search/?name={}", q);
    let body = fetch_html(&url).await.map_err(|e| err_response(404, &e))?;
    let html = Html::parse_document(&body);
    Ok(ok_response(parse_sounds(&html)))
}

#[get("/best?<q>")]
pub async fn best(q: String) -> Result<Json<ApiResponse<Vec<Sound>>>, (Status, Json<ApiResponse<()>>)> {
    if q.is_empty() {
        return Err(err_response(404, "Query parameter 'q' is required, example: ?q=id"));
    }
    let url = format!("https://www.myinstants.com/en/best_of_all_time/{}", q);
    let body = fetch_html(&url).await.map_err(|e| err_response(404, &e))?;
    let html = Html::parse_document(&body);
    Ok(ok_response(parse_sounds(&html)))
}

#[get("/detail?<id>")]
pub async fn detail(id: String) -> Result<Json<ApiResponse<SoundDetail>>, (Status, Json<ApiResponse<()>>)> {
    if id.is_empty() {
        return Err(err_response(400, "Query parameter 'id' is required, example: ?id=akh-26815"));
    }
    let url = format!("https://www.myinstants.com/en/instant/{}", id);
    let body = fetch_html(&url).await.map_err(|e| err_response(404, &e))?;
    let html = Html::parse_document(&body);

    let h1_sel = Selector::parse("h1#instant-page-title").unwrap();
    let title = html.select(&h1_sel).next()
        .map(|e| e.text().collect::<String>().trim().to_string())
        .unwrap_or_default();

    let btn_sel = Selector::parse("button#instant-page-button-element").unwrap();
    let sound_url = html.select(&btn_sel).next()
        .and_then(|e| e.value().attr("data-url"))
        .map(|u| format!("{}{}", BASE_URL, u))
        .unwrap_or_default();

    let desc_sel = Selector::parse("div#instant-page-description p").unwrap();
    let description = html.select(&desc_sel).next()
        .map(|e| e.text().collect::<String>().trim().to_string())
        .unwrap_or_default();

    let tags_sel = Selector::parse("div#instant-page-tags a").unwrap();
    let tags: Vec<String> = html.select(&tags_sel)
        .map(|e| e.text().collect::<String>().trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    let fav_sel = Selector::parse("div#instant-page-likes b").unwrap();
    let favorites = html.select(&fav_sel).next()
        .map(|e| e.text().collect::<String>().replace(" users", "").trim().to_string())
        .unwrap_or_default();

    let author_sel = Selector::parse("div#instant-page-likes ~ div").unwrap();
    let author_divs: Vec<_> = html.select(&author_sel).collect();
    let (uploader_name, uploader_url, views) = if author_divs.len() > 1 {
        let elem = author_divs[1];
        let a_sel = Selector::parse("a").unwrap();
        let a = elem.select(&a_sel).next();
        let uname = a.map(|e| e.text().collect::<String>().trim().to_string()).unwrap_or_default();
        let uurl = a.and_then(|e| e.value().attr("href")).map(|h| format!("{}{}", BASE_URL, h)).unwrap_or_default();
        let full_text = elem.text().collect::<String>();
        let views_text = full_text.replace("views", "").trim().to_string();
        let views = views_text.replace(&format!("Uploaded by {} - ", &uname), "").trim().to_string();
        (uname, uurl, views)
    } else {
        (String::new(), String::new(), String::new())
    };

    let detail_url = format!("{}/en/instant/{}", BASE_URL, &id);
    Ok(ok_response(SoundDetail {
        id,
        url: detail_url,
        title,
        mp3: sound_url,
        description,
        tags,
        favorites,
        views,
        uploader: Uploader {
            username: uploader_name,
            url: uploader_url,
        },
    }))
}

#[get("/favorites?<username>")]
pub async fn favorites(username: String) -> Result<Json<ApiResponse<Vec<Sound>>>, (Status, Json<ApiResponse<()>>)> {
    if username.is_empty() {
        return Err(err_response(400, "Query parameter 'username' is required, example: ?username=hellmouz"));
    }
    let url = format!("https://www.myinstants.com/en/profile/{}", username);
    let body = fetch_html(&url).await.map_err(|e| err_response(404, &e))?;
    let html = Html::parse_document(&body);
    Ok(ok_response(parse_sounds(&html)))
}

#[get("/uploaded?<username>")]
pub async fn uploaded(username: String) -> Result<Json<ApiResponse<Vec<Sound>>>, (Status, Json<ApiResponse<()>>)> {
    if username.is_empty() {
        return Err(err_response(400, "Query parameter 'username' is required, example: ?username=hellmouz"));
    }
    let url = format!("https://www.myinstants.com/en/profile/{}/uploaded/", username);
    let body = fetch_html(&url).await.map_err(|e| err_response(404, &e))?;
    let html = Html::parse_document(&body);
    Ok(ok_response(parse_sounds(&html)))
}
