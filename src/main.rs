use clap::Parser;
use core::panic;
use reqwest::Url;
use std::{num::ParseIntError, path::PathBuf};
use thiserror::Error;
use tokio::{
    fs,
    io::{self, AsyncWriteExt},
};

#[derive(Parser)]
#[command(author, version, about)]
struct CliArgs {
    ///the path to the fapello page you want to download images from
    ///(note: the homepage download is not supported)
    #[arg(long, short = 'u')]
    url: Url,
    ///path to the directory where file needs to be saved.
    ///if path does not exist it is recursively created
    #[arg(long, short = 'p', default_value = "./")]
    path: PathBuf,
}

const FAPELLO_BASE_URL: &str = "https://fapello.com/";

#[tokio::main]
async fn main() {
    let cli_args = CliArgs::parse();
    let save_location = cli_args.path;
    if !save_location.is_dir() {
        println!("save folder path doesnt exist attempting to create...");
        match fs::create_dir_all(&save_location).await {
            Ok(_) => {
                println!("save path folder created")
            }
            Err(err) => {
                panic!("{}", err)
            }
        };
    }
    //TODO prolly should add method for multiple urls
    let mut url = cli_args.url;
    if url.scheme() != "https" {
        url.set_scheme("https")
            .expect(format!("cannot set https scheme for '{url}'").as_str());
    }
    {
        let urlstr = url.to_string();
        if !urlstr.starts_with(FAPELLO_BASE_URL) {
            panic!("{url} is not a valid fapello url")
        }
        if urlstr == FAPELLO_BASE_URL {
            panic!("downloading from homepage is not supported")
        }
    }
    //this is the last id on fapello aka the first post in the grid.
    //there cant be ids bigger than this so yeah going from 1 to this good enough
    let largest_id = get_latest_id(url.clone()).await.unwrap();

    //hate needing to do conversions like this one but eh
    let vec_size = largest_id.try_into().unwrap();

    //contains all the urls for images that are going to be saved to disk
    let mut image_urls = Vec::with_capacity(vec_size);

    //put this in a diff block cause wont rlly need url_handlers once this block is done
    {
        let mut url_handles = Vec::with_capacity(vec_size);

        //creates all image post urls from 1 to the last id
        for i in 1..(largest_id + 1) {
            let url = url.clone().join(&i.to_string()).unwrap();
            url_handles.push(tokio::spawn(get_image_url(url.clone())));
        }

        //welp time to await i should prolly add fetching images in the same function but fuck it we ball :)
        for handle in url_handles {
            match handle.await.unwrap() {
                Ok(val) => image_urls.push(val),
                Err(err) => {
                    if matches!(err, GetImageUrlErrors::PostDoesntExist(_)) {
                        println!("image #{:?} doesnt exist", err)
                    }
                }
            };
        }
    }
    //welp time to download yay
    let mut download_handlers = Vec::new();
    for url in image_urls {
        download_handlers.push(downlaod_image(url, save_location.clone()))
    }
    for handle in download_handlers {
        handle.await.unwrap()
    }

    println!("download complete");
}

#[derive(Debug, Error)]
enum DownloadImageErrors {
    #[error("io error: {0}")]
    IoErrors(#[from] io::Error),
    #[error("web request error: {0}")]
    RequestError(#[from] reqwest::Error),
}

/**
 * basic function just creates the file and copies the bytes from reqwest
 * thankfully this wasnt mind numbing to figure out (lie) :)
 * TODO create struct for errors again and improve error handling
 */
async fn downlaod_image(url: String, base_path: PathBuf) -> Result<(), DownloadImageErrors> {
    let filename = url.split("/").last().unwrap();
    let filepath = base_path.join(filename);
    let mut file = fs::File::create(filepath).await?;
    let mut img_data = reqwest::get(url.clone()).await?.bytes().await?;
    file.write_all_buf(&mut img_data).await?;
    println!("saved file: {filename}");
    Ok(())
}

#[derive(Debug, Error)]
enum GetImageUrlErrors {
    #[error("web request error: {0}")]
    RequestError(#[from] reqwest::Error),
    #[error("post {0} does not exist")]
    PostDoesntExist(u16),
}

/** this function parses the post html and gets the image url */
async fn get_image_url(url: Url) -> Result<String, GetImageUrlErrors> {
    let res = reqwest::get(url.clone()).await?;
    if res.url().to_string() == FAPELLO_BASE_URL {
        let post_num: u16 = url.to_string().split("/").last().unwrap().parse().unwrap();
        return Err(GetImageUrlErrors::PostDoesntExist(post_num));
    }
    let html = res.text().await?;
    let dom = tl::parse(&html, tl::ParserOptions::default()).unwrap();
    let mut qs_iter = dom.query_selector("a.uk-align-center").unwrap();
    let handle = qs_iter.next().unwrap();
    let node = handle.get(dom.parser()).unwrap();
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

#[derive(Debug, Error)]
enum GetLatestIdErrors {
    #[error("web request error: {0}")]
    RequestError(#[from] reqwest::Error),
    #[error("url '{0}' does not exist")]
    DoesNotExistError(String),
    #[error("parse error {0}")]
    IdParseError(#[from] ParseIntError),
}

/** returns the id of the latest image uploaded on fapello
 * TODO use struct for error */
async fn get_latest_id(url: Url) -> Result<u64, GetLatestIdErrors> {
    let res = reqwest::get(url.clone()).await?;
    //if there is a redirect to home page it means the requested page was not found
    // why is this site like this why couldnt they just give us a 404 ;-;
    if res.url().to_string() == FAPELLO_BASE_URL {
        return Err(GetLatestIdErrors::DoesNotExistError(url.to_string()));
    }
    let html = res.text().await?;
    /* idk why but i cant just use query selector the same way i can for js
    prolly smth to do with settings but ill figure that out later */
    let dom = tl::parse(&html, tl::ParserOptions::default()).unwrap();
    //this is a hell hole and a tumour to js select #content > div > a
    //there is def a better way to do this idk how tho
    //there are too many unwraps and magic numbers needed cause there are random whitespaces bruh
    //prolly strip the whitespaces before parsing but this works too
    let node = dom
        .query_selector("#content")
        .and_then(|mut iter| iter.next())
        .unwrap()
        .get(dom.parser())
        .unwrap()
        .children()
        .unwrap()
        .all(dom.parser())
        .get(1)
        .unwrap()
        .as_tag()
        .unwrap()
        .children()
        .all(dom.parser())
        .get(1)
        .unwrap();
    //after the hell hole its simple enough to js extract the link from the href
    let link = node
        .as_tag()
        .unwrap()
        .attributes()
        .get("href")
        .unwrap()
        .unwrap()
        .try_as_utf8_str()
        .unwrap();
    //do i rlly need to create a vec to get the 2nd last element (its the id) here? prolly no
    //wondering why id is 2nd last and not last?
    //cause there is a slash in the end so last element is an empty string
    //but fuck it we ball
    let link_parts = link.split("/").collect::<Vec<_>>();
    let last_id: u64 = link_parts.get(link_parts.len() - 2).unwrap().parse()?;
    return Ok(last_id);
}
