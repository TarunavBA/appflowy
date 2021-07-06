pub use flowy_sdk::*;
use flowy_sys::prelude::*;
use std::{
    convert::TryFrom,
    fmt::{Debug, Display},
    fs,
    hash::Hash,
    path::PathBuf,
    sync::Once,
};

pub mod prelude {
    pub use crate::EventTester;
    pub use flowy_sys::prelude::*;
    pub use std::convert::TryFrom;
}

static INIT: Once = Once::new();
pub fn init_sdk() {
    let root_dir = root_dir();

    INIT.call_once(|| {
        FlowySDK::init_log(&root_dir);
    });
    FlowySDK::init(&root_dir);
}

fn root_dir() -> String {
    // https://doc.rust-lang.org/cargo/reference/environment-variables.html
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or("./".to_owned());
    let mut path_buf = fs::canonicalize(&PathBuf::from(&manifest_dir)).unwrap();
    path_buf.pop(); // rust-lib
    path_buf.push("flowy-test");
    path_buf.push("temp");
    path_buf.push("flowy");

    let root_dir = path_buf.to_str().unwrap().to_string();
    if !std::path::Path::new(&root_dir).exists() {
        std::fs::create_dir_all(&root_dir).unwrap();
    }
    root_dir
}

pub struct EventTester {
    request: DispatchRequest,
    assert_status_code: Option<StatusCode>,
}

impl EventTester {
    pub fn new<E>(event: E) -> Self
    where
        E: Eq + Hash + Debug + Clone + Display,
    {
        init_sdk();
        let request = DispatchRequest::new(event);
        Self {
            request,
            assert_status_code: None,
        }
    }

    pub fn payload<P>(mut self, payload: P) -> Self
    where
        P: ToBytes,
    {
        self.request = self.request.payload(Data(payload));
        self
    }

    pub fn assert_status_code(mut self, status_code: StatusCode) -> Self {
        self.assert_status_code = Some(status_code);
        self
    }

    #[allow(dead_code)]
    pub async fn async_send<R>(self) -> R
    where
        R: FromBytes,
    {
        let resp = async_send(self.request).await;
        dbg!(&resp);
        data_from_response(&resp)
    }

    pub fn sync_send<R>(self) -> R
    where
        R: FromBytes,
    {
        let resp = sync_send(self.request);
        if let Some(status_code) = self.assert_status_code {
            assert_eq!(resp.status_code, status_code)
        }

        dbg!(&resp);
        data_from_response(&resp)
    }
}

fn data_from_response<R>(response: &EventResponse) -> R
where
    R: FromBytes,
{
    let result = <Data<R>>::try_from(&response.payload).unwrap().into_inner();
    result
}