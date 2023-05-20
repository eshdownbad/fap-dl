use core::panic;
use reqwest::Url;
use std::{env, error, path::PathBuf, str::FromStr};
use tokio::{fs, io::AsyncWriteExt};

#[tokio::main]
async fn main() {
    //TODO use clap or something for this
    let args: Vec<String> = env::args().collect();
    let save_location_arg = args
        .get(2)
        .expect("2nd arg needs to be a path to save folder");
    let save_location = PathBuf::from(save_location_arg);
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
    //the first arg is the fapello url so yeah
    //TODO prolly should add method for multiple urls
    let url = Url::from_str(args.get(1).expect("url not found")).expect("cannot parse url");

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


/**
 * basic function just creates the file and copies the bytes from reqwest
 * thankfully this wasnt mind numbing to figure out (lie) :)
 * TODO create struct for errors again and improve error handling
 */
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

/** this function parses the post html and gets the image url */
async fn get_image_url(url: Url) -> Result<String, GetImageUrlErrors> {
    let res = reqwest::get(url.clone()).await?;
    if res.url().to_string() == "https://fapello.com/" {
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

/** returns the id of the latest image uploaded on fapello
 * TODO use struct for error */
async fn get_latest_id(url: Url) -> Result<u64, Box<dyn error::Error>> {
    let res = reqwest::get(url.clone()).await?;
    //if there is a redirect to home page it means the requested page was not found
    // why is this site like this why couldnt they just give us a 404 ;-;
    if res.url().to_string() == "https://fapello.com/" {
        //TODO create error type for this
        panic!("{url} does not exist :(")
    }
    let html = res.text().await?;
    /* idk why but i cant just use query selector the same way i can for js
    prolly smth to do with settings but ill figure that out later */
    let dom = tl::parse(&html, tl::ParserOptions::default()).unwrap();
    // this is a hell hole and a tumour to js select #content > div > a
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
    println!("{:?}", link_parts.get(link_parts.len() - 2));
    let last_id: u64 = link_parts.get(link_parts.len() - 2).unwrap().parse()?;
    return Ok(last_id);
}
