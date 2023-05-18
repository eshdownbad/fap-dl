use core::panic;
use reqwest::Url;
use std::{env, error, path::PathBuf, str::FromStr};
use tokio::{fs, io::AsyncWriteExt};

#[tokio::main]
async fn main() {
    let args: Vec<String> = env::args().collect();
    let url = Url::from_str(args.get(1).expect("url not found")).expect("cannot parse url");

    println!("fetching image count...");
    let count: u16;
    match get_count(url.clone()).await {
        Ok(val) => count = val.expect("post count could not be retrieved"),
        Err(e) => panic!("error: {}", e),
    }
    println!("posts count: {count}");
    let mut download_count: u16 = 0;
    while count > download_count {
        let mut range_start = 1;
        let mut range_end = count;
        if download_count > 0 {
            range_start = download_count + 1;
            println!("{count} , {download_count}");
            range_end = (count - download_count) + count;
        }

        let mut image_urls: Vec<String> = Vec::new();

        println!("fetching image urls...");

        let mut url_handles = Vec::new();

        for num in range_start..range_end + 1 {
            let url = url.clone().join(&num.to_string()).unwrap();
            url_handles.push(tokio::spawn(get_image_url(url.clone())));
        }

        for handle in url_handles {
            match handle.await.unwrap() {
                Ok(val) => {
                    download_count += 1;
                    image_urls.push(val)
                }
                Err(err) => {
                    if matches!(err, GetImageUrlErrors::PostDoesntExist(_)) {
                        println!("image #{:?} doesnt exist", err)
                    }
                }
            };
        }
        //TODO create an arg for this
        let base_path = PathBuf::from("./tmp");
        let mut download_handlers = Vec::new();
        for url in image_urls {
            download_handlers.push(downlaod_image(url, base_path.clone()))
        }
        for handle in download_handlers {
            handle.await.unwrap()
        }
    }
    println!("downloaded {download_count} images");
}

async fn downlaod_image(
    url: String,
    base_path: PathBuf,
) -> Result<(), Box<dyn error::Error + Sync + Send>> {
    let filename = url.split("/").last().unwrap();
    let filepath = base_path.join(filename);
    let mut file = fs::File::create(filepath).await?;
    let mut img_data = reqwest::get(url.clone()).await?.bytes().await?;
    file.write_all_buf(&mut img_data).await?;
    println!("saved file: {filename}");
    Ok(())
}

#[derive(Debug, Clone)]
struct PostDoesntExist;

#[derive(Debug)]
enum GetImageUrlErrors {
    RequestError(reqwest::Error),
    PostDoesntExist(u16),
}

impl From<reqwest::Error> for GetImageUrlErrors {
    fn from(err: reqwest::Error) -> Self {
        Self::RequestError(err)
    }
}

async fn get_image_url(url: Url) -> Result<String, GetImageUrlErrors> {
    let html = reqwest::get(url.clone()).await?.text().await?;
    let dom = tl::parse(&html, tl::ParserOptions::default()).unwrap();
    let mut qs_iter = dom.query_selector("a.uk-align-center").unwrap();
    let handle = qs_iter.next();
    //sometimes images are deleted from the site so this exists for that
    if handle == None {
        let post_num: u16 = url.to_string().split("/").last().unwrap().parse().unwrap();
        return Err(GetImageUrlErrors::PostDoesntExist(post_num));
    }
    let node = handle.unwrap().get(dom.parser()).unwrap();
    let link = node
        .as_tag()
        .unwrap()
        .attributes()
        .get("href")
        .unwrap()
        .unwrap()
        .try_as_utf8_str()
        .unwrap();

    Ok(String::from(link))
}

async fn get_count(url: Url) -> Result<Option<u16>, Box<dyn error::Error>> {
    let res = reqwest::get(url.clone()).await?;
    //if there is a redirect to main page that means the page was not found
    if res.url().to_string() == "https://fapello.com/" {
        return Ok(None);
    }
    //selcts and extracts the post count  from the html
    //there is a def a better way to do this
    let html = res.text().await?;
    let dom = tl::parse(&html, tl::ParserOptions::default()).unwrap();
    let  handle = dom.query_selector("div.divide-gray-300.divide-transparent.divide-x.grid.grid-cols-2.lg:text-left.lg:text-lg.mt-3.text-center.w-full.dark:text-gray-100")
    .and_then(|mut iter| iter.next()).unwrap();

    let node = handle.get(dom.parser()).unwrap().inner_text(dom.parser());
    let count = node.split(" ").nth(1);
    if count == None {
        return Ok(None);
    }
    return Ok(Some(count.unwrap().parse::<u16>()?));
}
