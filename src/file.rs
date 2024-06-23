use std::{collections::HashSet, error::Error, path::PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct FileBlocks {
    pub id: usize,
    pub block_size: usize,
    pub block_num: usize,
    #[serde(skip)]
    pub blocks: Vec<FileBlock>,
    #[serde(skip)]
    pub remaining: HashSet<usize>,
}
impl FileBlocks {
    pub fn new(id: usize) -> Self {
        Self {
            id,
            block_size: 60 * 1024,
            block_num: 0,
            blocks: vec![],
            remaining: HashSet::new(),
        }
    }
    pub fn info(&self) -> Self {
        FileBlocks {
            id: self.id,
            block_size: self.block_size,
            block_num: self.block_num,
            blocks: vec![],
            remaining: self.remaining.clone(),
        }
    }
    pub fn is_valid(&self) -> bool {
        self.id != 0
    }
    pub fn init(&mut self) {
        self.remaining = (0..self.block_num).collect();
        self.blocks.resize(self.block_num, FileBlock::DEFAULT);
    }
    pub fn load(&mut self, data: Vec<u8>) {
        let block_num = (data.len() - 1) / self.block_size + 1;
        self.block_num = block_num;
        for i in 0..block_num {
            let start = i * self.block_size;
            let end = ((i + 1) * self.block_size).min(data.len());
            let block = FileBlock {
                file_id: self.id,
                index: i,
                data: data[start..end].to_vec(),
            };
            self.blocks.push(block);
        }
        self.remaining = (0..block_num).collect();
    }
    pub fn get(&self, index: usize) -> Vec<u8> {
        self.blocks.get(index).unwrap_or(&FileBlock::DEFAULT).into()
    }
    pub fn done(&mut self, index: usize) -> bool {
        self.remaining.remove(&index)
    }
    pub fn set(&mut self, fb: FileBlock) {
        let index = fb.index;
        if fb.file_id == self.id && index < self.block_num {
            self.blocks[index] = fb;
            self.remaining.remove(&index);
        }
    }
    pub fn is_finished(&self) -> bool {
        self.remaining.is_empty()
    }
    pub fn save(self, fs: &FileState) {
        let data: Vec<u8> = self.blocks.into_iter().map(|b| b.data).flatten().collect();
        let path = fs.get_path();
        std::fs::write(path, data).unwrap();
    }
}
impl Into<Vec<u8>> for &FileBlocks {
    fn into(self) -> Vec<u8> {
        bincode::serialize(self).unwrap_or_default()
    }
}
impl From<&Vec<u8>> for FileBlocks {
    fn from(data: &Vec<u8>) -> Self {
        let mut res: FileBlocks = bincode::deserialize(data).unwrap_or_default();
        if res.is_valid() {
            res.blocks.resize(res.block_num, FileBlock::DEFAULT);
            res.remaining = (0..res.block_num).collect();
        }
        res
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct FileBlock {
    pub file_id: usize,
    pub index: usize,
    pub data: Vec<u8>,
}
impl FileBlock {
    pub const DEFAULT: Self = FileBlock {
        file_id: 0,
        index: 0,
        data: Vec::new(),
    };
    pub fn is_valid(&self) -> bool {
        self.file_id != 0
    }
}
impl Into<Vec<u8>> for &FileBlock {
    fn into(self) -> Vec<u8> {
        bincode::serialize(self).unwrap_or_default()
    }
}
impl From<&Vec<u8>> for FileBlock {
    fn from(data: &Vec<u8>) -> Self {
        bincode::deserialize(data).unwrap_or_default()
    }
}

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

impl FileState {
    pub fn get_path(&self) -> PathBuf {
        // if self.f.is_synced {
        // } else
        if !self.is_local {
            PathBuf::from(format!("{}/{}", "./downloads", self.name))
        } else if let Some(path) = &self.is_linked {
            path.clone()
        } else {
            PathBuf::from(format!("{}/{}", "./.file-net", self.name))
        }
    }
    pub fn get(&self) -> Result<Vec<u8>, Box<dyn Error>> {
        Ok(std::fs::read(self.get_path())?)
    }
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

#[cfg(test)]
mod test {
    use crate::file::*;

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

    #[derive(Debug, Default)]
    struct MyStruct {
        data: String,
    }

    #[test]
    fn test2() {
        let len = 5; // 预留的空间大小

        // 初始化一个空的 Vec<MyStruct>
        let mut vec: Vec<MyStruct> = Vec::new();

        // 预留空间，避免频繁的重新分配内存
        vec.resize_with(len, MyStruct::default);

        // 创建要插入的 MyStruct
        let my_struct_to_insert = MyStruct {
            data: "Example Data".to_string(),
        };

        // 指定要插入的位置
        let pos = 2;

        // 使用索引操作符将 my_struct_to_insert 插入到指定位置
        vec[3] = MyStruct {
            data: "Example ssss".to_string(),
        };
        vec.insert(pos, my_struct_to_insert);

        println!("{:?}", vec);
    }
}
