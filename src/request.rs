use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Job {
    pub url: String,
    pub headers: Value,
    pub body: Value,
    pub method: Method,
    pub params: Value,
    pub init_seq_num: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Request {
    pub url: String,
    pub headers: Value,
    pub body: Value,
    pub method: Method,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub enum Method {
    #[default]
    Post,
    Get,
    Put,
    Delete,
}

impl Job {
    #[inline]
    pub fn get(&self, index: u64) -> Request {
        let request = Request {
            url: self.url.clone(),
            headers: self.headers.clone(),
            body: self.body.clone(),
            method: self.method,
        };

        let request_json = serde_json::to_string(&request).unwrap();
        let mut result_json = request_json.clone();
        let re = Regex::new(r"<([^']+)>").unwrap();
        for gen in re.captures_iter(&request_json) {
            let (full, [gen_str]) = gen.extract();
            let gen = gen_str.split(':').collect::<Vec<&str>>();
            let name = gen[0];
            let gen_type = gen[1];
            let param = self.params.get(name);
            match gen_type {
                "SeqNum" => {
                    let (num, step) = if let Some(seq_param) = param {
                        let num = &seq_param["init_seq_num"];
                        let step = &seq_param["step"];
                        (
                            num.as_u64().unwrap_or(self.init_seq_num),
                            step.as_u64().unwrap_or(1),
                        )
                    } else {
                        (self.init_seq_num, 1)
                    };
                    let value = num + (step * index);
                    result_json = result_json.replace(full, &value.to_string());
                }
                "UUID" => {}
                _ => {}
            }
        }
        serde_json::from_str(&result_json).unwrap()
    }
}

#[test]
fn test_job_next() {
    common_x::log::init_log_filter("info");
    let job: Job = common_x::configure::file_config("job.toml").unwrap();
    let request = job.get(1);
    info!("request: {request:?}");
    info!("job: {job:?}");
}
