extern crate android_logger;
extern crate apply;
extern crate catch_panic;
extern crate jni_fn;
extern crate jnix;
extern crate jnix_macros;
extern crate log;
extern crate once_cell;
extern crate quick_xml;
extern crate regex;
extern crate tl;

use android_logger::Config;
use apply::Also;
use catch_panic::catch_panic;
use jni_fn::jni_fn;
use jnix::jni::objects::{JClass, JString};
use jnix::jni::sys::{jint, jintArray, jobject, jobjectArray, JavaVM, JNI_VERSION_1_6};
use jnix::jni::JNIEnv;
use jnix::{IntoJava, JnixEnv};
use jnix_macros::IntoJava;
use log::{error, LevelFilter};
use quick_xml::escape::unescape;
use std::ffi::c_void;
use tl::{Node, Parser, VDom};

macro_rules! regex {
    ($re:literal $(,)?) => {{
        static RE: once_cell::sync::OnceCell<regex::Regex> = once_cell::sync::OnceCell::new();
        RE.get_or_init(|| regex::Regex::new($re).unwrap())
    }};
}

const EHGT_PREFIX: &str = "https://ehgt.org/";
const EX_PREFIX: &str = "https://s.exhentai.org/";

trait Anon {
    fn get_first_element_by_class_name(&self, name: &str) -> Option<&Node>;
}

impl<'a> Anon for VDom<'a> {
    fn get_first_element_by_class_name(&self, name: &str) -> Option<&Node> {
        let handle = self.get_elements_by_class_name(name).next()?;
        Some(handle.get(self.parser())?)
    }
}

#[derive(Default, IntoJava)]
#[jnix(package = "com.hippo.ehviewer.client.parser")]
pub struct Torrent {
    posted: String,
    size: String,
    seeds: i32,
    peers: i32,
    downloads: i32,
    url: String,
    name: String,
}

#[derive(Default, IntoJava)]
#[allow(non_snake_case)]
#[jnix(package = "com.hippo.ehviewer.client.data")]
pub struct BaseGalleryInfo {
    gid: i64,
    token: String,
    title: String,
    titleJpn: Option<String>,
    thumbKey: String,
    category: i32,
    posted: String,
    uploader: Option<String>,
    disowned: bool,
    rating: f32,
    rated: bool,
    simpleTags: Vec<String>,
    pages: i32,
    thumbWidth: i32,
    thumbHeight: i32,
    spanSize: i32,
    spanIndex: i32,
    spanGroupIndex: i32,
    simpleLanguage: String,
    favoriteSlot: i32,
    favoriteName: Option<String>,
}

#[derive(Default, IntoJava)]
#[jnix(package = "com.hippo.ehviewer.client.parser")]
pub struct TorrentResult {
    list: Vec<Torrent>,
}

#[derive(Default, IntoJava)]
#[allow(non_snake_case)]
#[jnix(package = "com.hippo.ehviewer.client.parser")]
pub struct Limits {
    current: i32,
    maximum: i32,
    resetCost: i32,
}

fn parse_jni_string<F, R>(env: &mut JnixEnv, str: &JString, mut f: F) -> Option<R>
where
    F: FnMut(&VDom, &Parser, &JnixEnv) -> Option<R>,
{
    let html = env.get_string(*str).ok()?;
    let dom = tl::parse(html.to_str().ok()?, tl::ParserOptions::default()).ok()?;
    let parser = dom.parser();
    Some(f(&dom, parser, env)?)
}

#[no_mangle]
#[catch_panic(default = "std::ptr::null_mut()")]
#[allow(non_snake_case)]
#[jni_fn("com.hippo.ehviewer.client.parser.HomeParserKt")]
pub fn parseLimit(env: JNIEnv, _class: JClass, input: JString) -> jobject {
    let mut env = JnixEnv { env };
    let vec = parse_jni_string(&mut env, &input, |dom, parser, _env| {
        let iter = dom.query_selector("strong")?;
        let vec: Vec<i32> = iter
            .filter_map(|e| Some(e.get(parser)?.inner_text(parser).parse::<i32>().ok()?))
            .collect();
        if vec.len() == 3 {
            Some(Limits {
                current: vec[0],
                maximum: vec[1],
                resetCost: vec[2],
            })
        } else {
            None
        }
    })
    .unwrap();
    vec.into_java(&env).forget().into_raw()
}

#[no_mangle]
#[catch_panic(default = "std::ptr::null_mut()")]
#[allow(non_snake_case)]
#[jni_fn("com.hippo.ehviewer.client.parser.FavoritesParserKt")]
pub fn parseFav(env: JNIEnv, _class: JClass, input: JString, str: jobjectArray) -> jintArray {
    let mut env = JnixEnv { env };
    let vec = parse_jni_string(&mut env, &input, |dom, parser, env| {
        let fp = dom.get_elements_by_class_name("fp");
        let vec: Vec<i32> = fp
            .enumerate()
            .filter_map(|(i, e)| {
                if i == 10 {
                    return None;
                }
                let top = e.get(parser)?.children()?;
                let children = top.top();
                let cat = children[5].get(parser)?.inner_text(parser);
                let name = unescape(&cat).ok()?;
                env.set_object_array_element(str, i as i32, env.new_string(name.trim()).ok()?)
                    .ok()?;
                Some(
                    children[1]
                        .get(parser)?
                        .inner_text(parser)
                        .parse::<i32>()
                        .ok()?,
                )
            })
            .collect();
        if vec.len() == 10 {
            Some(vec)
        } else {
            None
        }
    })
    .unwrap_or(vec![]);
    env.new_int_array(10)
        .unwrap()
        .also(|it| env.set_int_array_region(*it, 0, &vec).unwrap())
}

#[no_mangle]
#[catch_panic(default = "std::ptr::null_mut()")]
#[allow(non_snake_case)]
#[jni_fn("com.hippo.ehviewer.client.parser.TorrentParserKt")]
pub fn parseTorrent(env: JNIEnv, _class: JClass, input: JString) -> jobject {
    let mut env = JnixEnv { env };
    parse_jni_string(&mut env, &input, |dom, parser, _env| {
        Some(TorrentResult {
            list: dom.query_selector("table")?.filter_map(|e| {
                let html = e.get(parser)?.inner_html(parser);
                let reg = regex!("</span> ([0-9-]+) [0-9:]+</td>[\\s\\S]+</span> ([0-9.]+ [KMGT]B)</td>[\\s\\S]+</span> ([0-9]+)</td>[\\s\\S]+</span> ([0-9]+)</td>[\\s\\S]+</span> ([0-9]+)</td>[\\s\\S]+</span>([^<]+)</td>[\\s\\S]+onclick=\"document.location='([^\"]+)'[^<]+>([^<]+)</a>");
                let grp = reg.captures(&html)?;
                let name = unescape(&grp[8]).ok()?;
                Some(Torrent {
                    posted: grp[1].to_string(),
                    size: grp[2].to_string(),
                    seeds: grp[3].parse().ok()?,
                    peers: grp[4].parse().ok()?,
                    downloads: grp[5].parse().ok()?,
                    url: grp[7].to_string(),
                    name: name.to_string()
                })
            }).collect()
        })
    }).unwrap().into_java(&env).forget().into_raw()
}

#[no_mangle]
#[catch_panic(default = "std::ptr::null_mut()")]
#[allow(non_snake_case)]
#[jni_fn("com.hippo.ehviewer.client.parser.GalleryListParserKt")]
pub fn parseGalleryInfo(env: JNIEnv, _class: JClass, input: JString) -> jobject {
    let mut env = JnixEnv { env };
    parse_jni_string(&mut env, &input, |dom, parser, _env| {
        let title = dom
            .get_first_element_by_class_name("glink")?
            .inner_text(parser)
            .into_owned();
        let thumb = dom
            .get_first_element_by_class_name("glthumb")?
            .children()?
            .top()
            .get(1)?
            .get(parser)?
            .as_tag()?
            .children()
            .top()
            .get(1)?
            .get(parser)?
            .as_tag()?
            .attributes()
            .get("data-src")??
            .try_as_utf8_str()?;
        error!("{}", thumb);
        Some(BaseGalleryInfo {
            gid: 0,
            token: "".to_string(),
            title: title.to_string(),
            titleJpn: None,
            thumbKey: thumb
                .trim_start_matches(EHGT_PREFIX)
                .trim_start_matches(EX_PREFIX)
                .trim_start_matches("t/")
                .to_string(),
            category: 0,
            posted: "".to_string(),
            uploader: None,
            disowned: false,
            rating: 0.0,
            rated: false,
            simpleTags: vec![],
            pages: 0,
            thumbWidth: 0,
            thumbHeight: 0,
            spanSize: 0,
            spanIndex: 0,
            spanGroupIndex: 0,
            simpleLanguage: "".to_string(),
            favoriteSlot: 0,
            favoriteName: None,
        })
    })
    .unwrap()
    .into_java(&env)
    .forget()
    .into_raw()
}

#[no_mangle]
pub extern "system" fn JNI_OnLoad(_: JavaVM, _: *mut c_void) -> jint {
    android_logger::init_once(Config::default().with_max_level(LevelFilter::Trace));
    JNI_VERSION_1_6
}
