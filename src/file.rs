use std::{error::Error, path::PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug)]
pub struct FileManager {
    /// state of files to list
    pub files: Vec<FileStateExtend>,
    /// path to which files are saved.
    pub storage: PathBuf,
    /// path to save structure.
    pub structure: PathBuf,

    /// which floder we want to show
    pub current: PathBuf,

    pub current_files: Vec<FileStateExtend>,
}

#[derive(Debug, Clone)]
pub struct FileStateExtend {
    pub f: FileState,
    pub is_selected: bool,
}

impl FileStateExtend {
    pub fn get(&self) -> Result<Vec<u8>, Box<dyn Error>> {
        // if self.f.is_synced {
        // } else
        if let Some(path) = &self.f.is_linked {
            Ok(std::fs::read(path)?)
        } else {
            Err("无法加载文件".into())
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct FileState {
    /// 是否是文件夹
    pub is_folder: bool,
    /// 是否指向文件系统目录，如果不是，则在 ./.file-net/{current}/ 目录下
    pub is_linked: Option<PathBuf>,
    /// 是否为本地文件
    pub is_local: bool,
    /// 是否在文件夹下存在（如为本地，则是否复制了；如为网络，则是否下载了）
    pub is_synced: bool,

    /// name of file in ./.file-net/.../
    pub name: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct FilesStructure {
    pub version: [usize; 2],
    pub files: Vec<FileState>,
}

impl FileManager {
    const VERSION: [usize; 2] = [0, 1];
    pub fn new() -> Self {
        let mut res = Self {
            files: vec![],
            storage: "./.file-net/".into(),
            structure: "./.file-net-struct/".into(),
            current: "".into(),
            current_files: vec![],
        };
        res.open_current();
        res
    }

    pub fn open_current(&mut self) {
        // read all as string from path
        let data = std::fs::read_to_string(self.get_struct_path()).unwrap_or_default();
        let structured_data = serde_json::from_str(&data);
        match structured_data {
            Ok(FilesStructure { version, files }) => {
                if version == Self::VERSION {
                    self.current_files = files
                        .into_iter()
                        .map(|f| FileStateExtend {
                            f,
                            is_selected: false,
                        })
                        .collect();
                } else {
                    println!("Dismatched version！");
                }
            }
            Err(e) => {
                println!("Cannot read from file! e: {e}");
            }
        }
    }

    pub fn list_files(&self) -> &Vec<FileStateExtend> {
        &self.current_files
    }

    pub fn write_files(&self) {
        let data = serde_json::to_vec_pretty(&FilesStructure {
            version: Self::VERSION,
            files: self.current_files.iter().map(|f| f.f.clone()).collect(),
        })
        .unwrap();
        std::fs::write(self.get_struct_path(), data).unwrap();
    }

    pub fn add_file(&mut self, f: FileStateExtend) {
        self.current_files.push(f);
        self.write_files();
    }

    fn get_struct_path(&self) -> PathBuf {
        let mut path: PathBuf = self.structure.clone();
        path.push(self.current.to_owned());
        path.push("struct.json");
        println!("Path: {:?}", path);
        path
    }
}

#[test]
fn test() {
    let mut fm = FileManager::new();
    // fm.write_files();
    let new_file = FileState {
        is_folder: false,
        is_linked: Some(
            "D:\\Program\\Rust\\Project\\file-net\\.github\\workflows\\create-git-release.js"
                .into(),
        ),
        is_local: true,
        is_synced: false,
        name: "git-release".to_owned(),
    };
    fm.add_file(FileStateExtend {
        f: new_file,
        is_selected: false,
    });
    fm.open_current();
    let res = fm.list_files();
    println!("{:#?}", res);
}
