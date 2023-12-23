use std::{env, ffi::OsStr, io::Write, path::Path};

use actix_multipart::Multipart;
use actix_web::{
    web::{self, Query},
    Error, HttpResponse,
};
use actix_web_validator::Json;
use futures::stream::TryStreamExt;
use log::info;
use mongodb::{
    bson::{doc, oid::ObjectId, DateTime, Regex},
    options::FindOptions,
    Client,
};
use uuid::Uuid;
use validator::Validate;
use reqwest;
use pulldown_cmark::{Parser, html::push_html};

use crate::{
    database::get_blog_collection,
    model::{
        blog::{Blog, BlogOneQuery, BlogQuery},
        helper::ResponseMessage,
        user::AuthorizedUser,
    },
};
use serde::{self, Deserialize, Serialize};


#[derive(Debug, Deserialize, Serialize, Validate)]
pub struct NotifyKey {
  pub key: String,
}

#[derive(Debug, Deserialize, Serialize, Validate)]
pub struct Poster2 {
    pub url: String,
    pub width: u16,
    pub height: u16,
}

#[derive(Debug, Deserialize, Serialize, Validate)]
pub struct Notify {
  pub _id: Option<ObjectId>,
  pub originalname: String,
  pub poster: String,
  pub poster2: Poster2,
  pub magnetUrl: String,
  pub screenshots: Vec<String>,
  pub previewvideo: Option<String>,
  pub created_at: Option<DateTime>
}

#[derive(Debug, Deserialize, Serialize, Validate)]
pub struct NotifyRes {
    pub movie: Notify
}

#[derive(Serialize, Deserialize, Debug)]
struct RequestData {
    text: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct ResponseData {
    text: String,
}

pub async fn notify( client: web::Data<Client>,
    query: web::Query<NotifyKey>,
    notify: Json<NotifyRes>) -> HttpResponse {
    let notify_key = env::var("NOTIFY_KEY").unwrap();
    if query.key != notify_key {
        return HttpResponse::Unauthorized().json(ResponseMessage {
            success: 0,
            message: "对不起，认证失败！".to_string(),
        });
    }
    let title = &notify.movie.originalname;
    let ai_generate = post_text(title).await.unwrap_or(ResponseData { text: "暂无内容".to_string() }).text;
    let content_tohtml = markdown_to_html(&ai_generate);
    let blog_db = get_blog_collection(client);
    let mut html_content = String::new();
    let host = env::var("EFV_HOST").unwrap();
    for url in &notify.movie.screenshots {
        html_content.push_str(&format!("<p><a href=\"{}{}\" target=\"_blank\">{}{}</a></p>", &host, url, &host, url));
    }
    let content = format!("{}<h3>预览视频</h3><p><a href=\"{}\" target=\"_blank\">预览视频</a></p> <h3>预览截图</h3> {}", &content_tohtml, host + &notify.movie.previewvideo.clone().unwrap_or("暂无预览视频".to_string()) ,html_content);
    let new_blog = Blog {
        created_at: Some(DateTime::now()),
        _id: Some(ObjectId::new()),
        title: notify.movie.originalname.clone(),
        content,
        resource: extract_btih_from_magnet(&notify.movie.magnetUrl),
    };

    let new_blog = blog_db.insert_one(new_blog, None).await;
    match new_blog {
        Ok(insert) => HttpResponse::Ok().json(PostResponse {
            success: 1,
            id: insert.inserted_id.as_object_id().unwrap().to_string(),
        }),
        Err(err) => HttpResponse::Ok().json(ResponseMessage {
            success: 0,
            message: err.to_string(),
        }),
    }
}

fn markdown_to_html(markdown_input: &str) -> String {
    let parser = Parser::new(markdown_input);
    let mut html_output = String::new();
    push_html(&mut html_output, parser);
    html_output
}

fn extract_btih_from_magnet(magnet_link: &str) -> Option<String> {
    let parts: Vec<&str> = magnet_link.split('&').collect();
    for part in parts {
        if part.starts_with("magnet:?xt=urn:btih:") {
            return part.strip_prefix("magnet:?xt=urn:btih:").map(|s| s.to_string());
        }
    }
    None
}

async fn post_text(text: &str) -> Result<ResponseData, reqwest::Error> {
    let client = reqwest::Client::new();
    let url = env::var("AI_URL").unwrap();

    let request_data = RequestData {
        text: text.to_string(),
    };

    let response = client.post(url)
        .json(&request_data)
        .send()
        .await?
        .json::<ResponseData>()
        .await?;

    Ok(response)
}

#[derive(Deserialize, Serialize)]
struct PostResponse {
    success: i8,
    id: String,
}
pub async fn create(
    client: web::Data<Client>,
    authorized_user: Option<AuthorizedUser>,
    blog: Json<Blog>,
) -> HttpResponse {
    if authorized_user.is_none() {
        return HttpResponse::Unauthorized().json(ResponseMessage {
            success: 0,
            message: "对不起，认证失败！".to_string(),
        });
    }
    let blog_db = get_blog_collection(client);
    let new_blog = Blog {
        created_at: Some(DateTime::now()),
        _id: Some(ObjectId::new()),
        ..blog.into_inner()
    };
    let new_blog = blog_db.insert_one(new_blog, None).await;
    match new_blog {
        Ok(insert) => HttpResponse::Ok().json(PostResponse {
            success: 1,
            id: insert.inserted_id.as_object_id().unwrap().to_string(),
        }),
        Err(err) => HttpResponse::Ok().json(ResponseMessage {
            success: 0,
            message: err.to_string(),
        }),
    }
}

pub async fn update(
    client: web::Data<Client>,
    authorized_user: Option<AuthorizedUser>,
    blog: Json<Blog>,
) -> HttpResponse {
    if authorized_user.is_none() {
        return HttpResponse::Unauthorized().json(ResponseMessage {
            success: 0,
            message: "对不起，认证失败！".to_string(),
        });
    }
    info!{"{:?}", blog};
    let blog_db = get_blog_collection(client);
    let update = blog_db
        .update_one(
            doc! {
                "_id": blog._id.unwrap()
            },
            doc! {
                "$set": {
                    "title": &blog.title,
                    "content": &blog.content,
                    "resource": &blog.resource
                }
            },
            None,
        )
        .await;
    match update {
        Ok(_) => {
            HttpResponse::Ok().json(PostResponse {
            success: 1,
            id: blog._id.unwrap().to_string(),
        })},
        Err(err) => HttpResponse::Ok().json(ResponseMessage {
            success: 0,
            message: err.to_string(),
        }),
    }
}

pub async fn get_blog(client: web::Data<Client>, query: web::Query<BlogOneQuery>) -> HttpResponse {
    let blog_db = get_blog_collection(client);
    let blog = blog_db
        .find_one(
            doc! {
              "_id": ObjectId::parse_str(query.id.to_string()).unwrap()
            },
            None,
        )
        .await;
    match blog {
        Ok(Some(b)) => HttpResponse::Ok().json(Some(b)),
        Ok(None) => HttpResponse::Ok().json(ResponseMessage {
            success: 0,
            message: "本博文已经不存在了！".to_string(),
        }),
        Err(err) => HttpResponse::Ok().json(ResponseMessage {
            success: 0,
            message: err.to_string(),
        }),
    }
}

pub async fn get_blogs(client: web::Data<Client>, query: Query<BlogQuery>) -> HttpResponse {
    let query_obj = query.into_inner();
    let mut find = doc! {};
    let limit = query_obj.limit as i64;
    let page = query_obj.page as i64;
    let find_options = FindOptions::builder()
        .sort(doc!{
            "created_at": -1
        })
        .limit(limit)
        .skip(((page - 1) * limit) as u64)
        .build();
    if !query_obj.q.is_none() {
        let regex = Regex {
            pattern: query_obj.q.unwrap(),
            options: "x".to_string(),
        };
        find = doc! {
          "title": regex,
        };
    }
    let blog_db = get_blog_collection(client);
    let blogs = blog_db.find(find, find_options).await;
    match blogs {
        Ok(mut bs) => {
            let mut blogs_list: Vec<Blog> = vec![];
            while let Ok(Some(b)) = bs.try_next().await {
                blogs_list.push(b);
            }
            HttpResponse::Ok().json(blogs_list)
        }
        Err(err) => HttpResponse::Ok().json(ResponseMessage {
            success: 0,
            message: err.to_string(),
        }),
    }
}

#[derive(Deserialize, Serialize)]
struct UploadResponse {
    errno: i8,
    data: ImageData,
}

#[derive(Deserialize, Serialize)]
struct ImageData {
    url: String,
    href: String,
}

pub async fn upload(
    mut payload: Multipart,
    authorized_user: Option<AuthorizedUser>,
) -> Result<HttpResponse, Error> {
    if authorized_user.is_none() {
        return Ok(HttpResponse::Unauthorized().json(ResponseMessage {
            success: 0,
            message: "对不起，认证失败！".to_string(),
        }));
    }
    if let Some(mut field) = payload.try_next().await? {
        let content_disposition = field.content_disposition();
        let mimetype = field.content_type();
        let lowercase_mimetype = mimetype.to_string().to_lowercase();
        if !lowercase_mimetype.starts_with("image") {
            return Ok(HttpResponse::Ok().json(ResponseMessage {
                success: 0,
                message: "仅能上传图片".to_string(),
            }));
        }
        let original_name = content_disposition.get_filename().unwrap();
        let ext_name = Path::new(original_name)
            .extension()
            .and_then(OsStr::to_str)
            .unwrap();
        let filename = Uuid::new_v4().to_string();
        let filepath = format!("./static/{filename}.{ext_name}");
        let url = filepath.replace("./static", "/static");
        // File::create is blocking operation, use threadpool
        let mut f = web::block(|| std::fs::File::create(filepath)).await??;

        // Field in turn is stream of *Bytes* object
        while let Some(chunk) = field.try_next().await? {
            // filesystem operations are blocking, we have to use threadpool
            f = web::block(move || f.write_all(&chunk).map(|_| f)).await??;
        }
        let mut host = env::var("HOST").unwrap();
        host.push_str(&url);
        Ok(HttpResponse::Ok().json(UploadResponse {
            errno: 0,
            data: ImageData {
                url: host.clone(),
                href: host,
            },
        }))
    } else {
        Ok(HttpResponse::BadRequest().json({
            ResponseMessage {
                success: 0,
                message: "上传失败，请重试！".to_string(),
            }
        }))
    }
}
