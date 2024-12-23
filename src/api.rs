use std::{env, fs::{self, OpenOptions}, io::{self, BufReader, Read, Write}, net::TcpStream, path::{Path, PathBuf}, process::{Command, Stdio}};
use anyhow::anyhow;
use crossterm::{cursor, terminal, ExecutableCommand};
use ftp::FtpStream;
use path_slash::{PathBufExt, PathExt};
use serde::{Deserialize, Serialize};
use ssh2::Session;
use walkdir::WalkDir;
use zip::ZipArchive;

use crate::cli::Arguments;

pub trait Executor {
	fn execute(&self) -> Result<bool, anyhow::Error>;
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct Admin {
	revision: String,
	previous: String,
	branch: String
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct Revision {
	admin: Admin
}

impl Revision {
	pub fn new(revision: &str, branch: &str) -> Revision {

		let admin = Admin {
			revision: String::from(revision),
			previous: String::from(revision),
			branch: String::from(branch)
		};

		Revision {
			admin
		}
	}
}

pub struct Export<'a> {
	pub session: ssh2::Session,
	pub args: Arguments,
	pub files: Vec<&'a str>
}

impl<'a> Export<'a> {
	
	pub fn new(mut args: Arguments) -> Result<Self, anyhow::Error> {

		let mut local_repo = args.local.clone();

		if local_repo.is_empty() {
			local_repo = env::current_dir().unwrap().to_string_lossy().to_string()
		}
		else {
			local_repo = shellexpand::full(&local_repo).unwrap().to_string();
		}

		args.local = local_repo;

		is_git_repo(&args)?;

		if args.dist {
			args.create = true;
		}

		let connection_str = format!("{}:{}", args.host, "22");

		let tcp = TcpStream::connect(connection_str)?;

		let mut session = Session::new()?;
		session.set_compress(true);
		session.set_tcp_stream(tcp);
	
		session.handshake()?;
		session.userauth_password(&args.user, &args.password)?;

		Ok(Self {
			session: session.clone(),
			args: args.clone(),
			files: vec![]
		})
	}

	pub fn get_revision_file(&self) -> Result<Revision, anyhow::Error> {
		
		let mut folder = self.args.destination.clone();

		if folder.starts_with('.') {
			let mut str = folder.to_string();
			str.remove(0);
			folder = str;
		}

		let file = format!("{}/revision.json", folder);

		let session = &self.session;

		let (mut remote_file, _) = session.scp_recv(&PathBuf::from(&file))?;

		let mut content = String::new();
		remote_file.read_to_string(&mut content)?;
	
		remote_file.send_eof()?;
		remote_file.wait_eof()?;
		remote_file.close()?;
		remote_file.wait_close()?;

		let revision = serde_json::from_str::<Revision>(&content)?;

		Ok(revision)
	}

	pub fn git_pull(&self) -> Result<(), anyhow::Error> {

		let args = &self.args;

		let local_path = PathBuf::from(&args.local);

		println!();
		println!("git pull");
		println!();

		let git_pull = Command::new("git")
					.current_dir(&local_path)
					.arg("pull")
					.stdout(Stdio::inherit())
					.stderr(Stdio::inherit())
					.output().unwrap();

		if !git_pull.status.success() {
			return Err(anyhow!(String::from_utf8(git_pull.stderr)?));
		}

		Ok(())
	}

	fn create_dist(&self) -> Result<PathBuf, anyhow::Error> {

		let args = &self.args;
		let local_repo = args.local.clone();
		let dest = args.destination.clone();
		let verbose = args.verbose;

		let pid = std::process::id();
		let temp_dir = env::temp_dir().to_slash().unwrap().to_string();

		let mut export_path = PathBuf::new();

		env::set_current_dir(&local_repo).unwrap();

		let dest_path = PathBuf::from(dest.clone());

		let dir_name = dest_path.file_name().unwrap();

		let folder_name = dir_name.to_str().unwrap();
		
		let mut deploy_path = PathBuf::new();

		deploy_path.push(temp_dir.clone());
		deploy_path.push(format!("deploy-{}", pid));

		let file_path = deploy_path.join(format!("{}.zip",folder_name));

		if file_path.exists() {

			fs::remove_file(file_path.clone()).unwrap();
		}

		if !deploy_path.exists(){

			fs::create_dir_all(deploy_path.as_path()).unwrap();
		}

		let head_output = Command::new("git")
			.arg("rev-parse")
			.arg("HEAD")
			.output().unwrap();

		let head_ref = String::from_utf8(head_output.stdout).unwrap();

		let head_ref = head_ref.trim_end();

		let cmd = format!("git archive -o {} HEAD", file_path.clone().to_str().unwrap());

		let cmd_output = if cfg!(target_os = "windows") {

			Command::new("powershell")
				.args(["-c", cmd.clone().as_str()])
				.stdout(Stdio::inherit())
				.output().unwrap()
		} else {
			Command::new("bash")
					.args(["-c", cmd.clone().as_str()])
					.stdout(Stdio::inherit())
					.output().unwrap()
		};

		let mut success = cmd_output.status.success();

		if success {

			let zip_file = fs::File::open(file_path.clone()).unwrap();

			let mut archive = ZipArchive::new(zip_file).unwrap();

			if verbose {
				println!();
				println!("Extracting {}", file_path.clone().to_str().unwrap());
				println!();
			}

			for i in 0..archive.len() {

				let mut file = archive.by_index(i).unwrap();

				let outpath = match file.enclosed_name() {
					Some(path) => path.to_owned(),
					None => continue,
				};

				let mut out_path = PathBuf::new();
				out_path.push(deploy_path.clone());
				out_path.push(folder_name);
				out_path.push(outpath.clone());
		
				if (file.name()).ends_with('/') {

					if verbose {
						println!("Creating directory: {}", out_path.clone().to_str().unwrap());
					}

					fs::create_dir_all(&out_path).unwrap();

				} else {
					if let Some(p) = out_path.parent() {
						if !p.exists() {
							
							if verbose {
								println!("Creating file: {}", file.name());
							}
								
							fs::create_dir_all(p).unwrap();
						}
					}
					let mut outfile = fs::File::create(&out_path).unwrap();
					io::copy(&mut file, &mut outfile).unwrap();
				}
			}

			fs::remove_file(file_path).unwrap();

			export_path.push(deploy_path.clone());
			export_path.push(folder_name);

			env::set_current_dir(&local_repo).unwrap();

			let branch_output = Command::new("git")
				.arg("rev-parse")
				.arg("--abbrev-ref")
				.arg("HEAD")
				.output().unwrap();

			success = branch_output.status.success();

			if success {
				
				env::set_current_dir(export_path.clone()).unwrap();

				let branch = String::from_utf8(branch_output.stdout).unwrap();

				let branch = branch.trim_end();

				if verbose {
					println!();
					println!("Creating revision.json");
				}
				
				let revision = Revision::new(head_ref, branch);

				let revision_json = serde_json::to_string(&revision).unwrap();

				let mut revision_path = export_path.clone();

				revision_path.push(deploy_path.clone());
				revision_path.push(folder_name);
				revision_path.push("revision.json");

				let mut revision_file = fs::OpenOptions::new().write(true).create_new(true).open(revision_path).unwrap();

				revision_file.write_all(revision_json.as_bytes()).unwrap();

				println!();
				println!("BRANCH: {}", &branch);
				println!("HEAD: {}", &head_ref);
				println!("SERVER: {}", &head_ref);
				println!("LOCAL: {}", export_path.clone().to_slash().unwrap());
			}
			else {
				return Err(anyhow!("Could not create revision file"));
			}
			
		}
		else {
			return Err(anyhow!("Could not create git archive"));
		}

		Ok(export_path)
	}

	fn create_export(&self) -> Result<PathBuf, anyhow::Error> {

		let args = &self.args;
		let local_repo = args.local.clone();
		let dest = args.destination.clone();
		let verbose = args.verbose;

		let pid = std::process::id();
		let temp_dir = env::temp_dir().to_slash().unwrap().to_string();

		let mut export_path = PathBuf::new();

		let revision_file_server = self.get_revision_file()?;

		env::set_current_dir(&local_repo)?;

		let dest_path = PathBuf::from(dest.clone());

		let dir_name = dest_path.file_name().unwrap();

		let folder_name = dir_name.to_str().unwrap();
		
		let mut deploy_path = PathBuf::new();

		deploy_path.push(temp_dir.clone());
		deploy_path.push(format!("deploy-{}", pid));

		let file_path = deploy_path.join(format!("{}.zip",folder_name));

		if file_path.exists() {

			fs::remove_file(file_path.clone())?;
		}

		if !deploy_path.exists(){

			fs::create_dir_all(deploy_path.as_path()).unwrap();
		}

		let head_output = Command::new("git")
			.arg("rev-parse")
			.arg("HEAD")
			.output().unwrap();

		let head_ref = String::from_utf8(head_output.stdout.clone()).unwrap();

		let head_ref = head_ref.trim_end();
		
		if head_ref != revision_file_server.admin.revision {

			let mut temp_list = PathBuf::new();

			temp_list.push(temp_dir);
			temp_list.push("list.txt");

			let temp_slashed = temp_list.to_slash().unwrap();

			let cmd = format!("git diff --name-only --diff-filter=d {} HEAD > {}", revision_file_server.admin.revision, temp_slashed.clone());

			let cmd_output = if cfg!(target_os = "windows") {

				Command::new("cmd")
					.args(["/C", cmd.clone().as_str()])
					.stdout(Stdio::inherit())
					.output().unwrap()
			} else {
				Command::new("sh")
						.args(["-c", cmd.clone().as_str()])
						.stdout(Stdio::inherit())
						.output().unwrap()
			};

			let mut success = cmd_output.status.success();

			if success && temp_list.exists() {

				let list_data = fs::read_to_string(&temp_list).unwrap();

				let lines = list_data.lines();

				if verbose {
					println!();
					println!("Extracting {}", file_path.clone().to_str().unwrap());
					println!();
				}
				

				for line in lines {

					let mut out_path = PathBuf::new();
					out_path.push(deploy_path.clone());
					out_path.push(folder_name);
					
					let mut file_path = out_path.clone();
					file_path.push(line);

					let parent_dir = file_path.parent().unwrap();
			
					if !&parent_dir.exists() {

						if verbose {
							println!("Creating path: {}", parent_dir.to_string_lossy());
						}

						fs::create_dir_all(parent_dir).unwrap();
					}

					out_path.push(line);

					let mut outfile = fs::File::create(&out_path).unwrap();

					let mut local_file_path = PathBuf::new();
					local_file_path.push(&local_repo);
					local_file_path.push(line);

					let mut file = fs::OpenOptions::new().read(true).open(local_file_path).unwrap();
					io::copy(&mut file, &mut outfile).unwrap();
				}

				if file_path.exists() {

					fs::remove_file(file_path.clone())?;
				}

				export_path.push(deploy_path.clone());
				export_path.push(folder_name);

				env::set_current_dir(&local_repo).unwrap();

				let branch_output = Command::new("git")
					.arg("rev-parse")
					.arg("--abbrev-ref")
					.arg("HEAD")
					.output().unwrap();

				success = branch_output.status.success();

				if success {

					env::set_current_dir(export_path.clone()).unwrap();

					let branch = String::from_utf8(branch_output.stdout).unwrap();

					let branch = branch.trim_end();

					if verbose {
						println!();
						println!("Creating revision.json");
					}

					let revision = Revision::new(head_ref, branch);

					let revision_json = serde_json::to_string(&revision).unwrap();

					let mut revision_path = export_path.clone();

					revision_path.push(deploy_path.clone());
					revision_path.push(folder_name);
					revision_path.push("revision.json");

					if revision_path.exists() {
						fs::remove_file(revision_path.clone()).unwrap();
					}

					let mut revision_file = fs::OpenOptions::new().write(true).create_new(true).open(revision_path).unwrap();

					revision_file.write_all(revision_json.as_bytes()).unwrap();

					println!();
					println!("BRANCH: {}", &branch);
					println!("HEAD: {}", &head_ref);
					println!("SERVER: {}", &revision_file_server.admin.revision);
					println!("LOCAL: {}", export_path.clone().to_slash().unwrap());

				}
			}
			else {
				return Err(anyhow!(format!("ERROR: {:?}", cmd_output)));
			}
		}
		else {
			return Err(anyhow!(format!("ERROR: {:?}", head_output)));
		}

		Ok(export_path)
	}

	#[warn(unused_assignments)]
	pub fn deploy(&self) -> Result<bool, anyhow::Error> {

		let args = &self.args;
		let mut dest = args.destination.clone();
		let verbose = args.verbose;
		let session = &self.session;

		let mut stdout = std::io::stdout();

		let dist = args.dist;
		let create = args.create;

		if dest.starts_with('.') {
			dest.remove(0);
		}

		let local_path = if dist || create {
			self.create_dist()?
		}
		else {
			self.create_export()?
		};

		let mut dest_path = PathBuf::new();

		dest_path.push(dest);

		if dist {
			let time_stamp = chrono::offset::Local::now().format("%Y%m%d-%H%M%S").to_string();

			dest_path.push(time_stamp);
		}

		let local_str = local_path.clone().to_string_lossy().to_string();

		let mut count :u64 = 0;
		let mut current: u64 = 0;

		for entry in WalkDir::new(local_path.clone()).into_iter().filter_map(|e| e.ok()) {

			let meta_data = entry.metadata().unwrap();

			if meta_data.is_file() {
				count += 1;
			}
		}

		println!();

		let mut counter = 3;

		for entry in WalkDir::new(local_path.clone()).into_iter().filter_map(|e| e.ok()) {

			let meta_data = entry.metadata().unwrap();

			let dest_export = entry.path().to_path_buf();

			let mut str_dest = String::from(dest_export.to_str().unwrap());

			str_dest = str_dest.replace(local_str.as_str(), dest_path.to_str().unwrap());

			let export_dest = str_dest.trim();

			let mut export_path = PathBuf::new();

			export_path.push(export_dest);

			let str_export = export_path.to_slash().unwrap().to_string();
			
			if meta_data.is_dir() {

				let mut cmd = Path::new(&str_export).to_slash_lossy().to_string();

				cmd = format!("cd / && mkdir -p {}", cmd);

				let mut channel = session.channel_session()?;

				let result = channel.exec(&cmd);

				match result {
					Ok(_) => {
						if verbose {
							println!("MKDIR: {}", str_export.clone())
						}
					}
					Err(error) => {
						
						if verbose {
							println!("Error: {}", error);
						}
					} 
				}
			}
			else if meta_data.is_file() {

				let entry_path = entry.path();

				let meta_data = entry.metadata().unwrap();

				let file_name = entry_path.file_name().unwrap().to_string_lossy().to_string().replace('"', "");

				let file_path = PathBuf::from(str_export);

				let dir_slashed = file_path.parent().unwrap().to_slash().unwrap().to_string();

				let mut file_path = PathBuf::new();

				file_path.push(&dir_slashed);
				file_path.push(&file_name);

				if current > 0 {

					let _ = stdout.execute(cursor::MoveUp(counter));
					let _ = stdout.execute(terminal::Clear(terminal::ClearType::FromCursorDown));
					counter = 3;
				}
				
				current += 1;

				let progress = ((current as f64 / count as f64) * 100.0) as i32;

				writeln!(stdout, "Deploying: {progress}%").unwrap();
				writeln!(stdout, "{file_name}").unwrap();
				writeln!(stdout, "{current} / {count}").unwrap();

				let mut scp = session.scp_send(&file_path, 0o751, meta_data.len(), None)?;

				let file = fs::OpenOptions::new().read(true).open(entry_path).unwrap();

				let mut reader = BufReader::new(file);

				let mut handle = [0; 4096];

				loop {
					
					let len = reader.read(&mut handle).unwrap();

					if len == 0  {
						break;
					}

					let _ = scp.write_all(&handle[..len]);

				}
				
			}
		}

		Ok(true)
	}
}

impl<'a> Executor for Export<'a> {
	fn execute(&self) -> Result<bool, anyhow::Error> {
		
		self.git_pull()?;

		self.deploy()
	}
}

fn is_git_repo(args: &Arguments) -> Result<bool, anyhow::Error> {

	let local_path = PathBuf::from(&args.local);

	let repo_output = Command::new("git")
		.current_dir(&local_path)
		.arg("rev-parse")
		.arg("--is-inside-work-tree")
		.output().unwrap();

	let is_repo = String::from_utf8(repo_output.stdout).unwrap();

	let is_repo = is_repo.trim_end();

	if is_repo != "true" {
		Err(anyhow!("This can only be run in a git repo..."))
	}
	else {
		Ok(true)
	}
}

pub struct FtpExport<'a> {
	pub args: Arguments,
	pub files: Vec<&'a str>
}

impl<'a> FtpExport<'a> {

	pub fn new(mut args: Arguments) -> Result<Self, anyhow::Error> {

		let mut local_repo = args.local.clone();

		if local_repo.is_empty() {
			local_repo = env::current_dir().unwrap().to_string_lossy().to_string()
		}
		else {
			local_repo = shellexpand::full(&local_repo).unwrap().to_string();
		}

		args.local = local_repo;

		is_git_repo(&args)?;

		if args.dist {
			args.create = true;
		}

		Ok(Self {
			args: args.clone(),
			files: vec![]
		})
	}

	fn git_pull(&self) -> Result<(), anyhow::Error> {

		let args = &self.args;

		let local_path = PathBuf::from(&args.local);

		println!();
		println!("git pull");
		println!();

		let git_pull = Command::new("git")
					.current_dir(&local_path)
					.arg("pull")
					.stdout(Stdio::inherit())
					.stderr(Stdio::inherit())
					.output().unwrap();

		if !git_pull.status.success() {
			return Err(anyhow!(String::from_utf8(git_pull.stderr)?));
		}

		Ok(())
	}

	fn get_revision_file(&self) -> Result<Revision, anyhow::Error> {
		
		let mut folder = self.args.destination.clone();

		if folder.starts_with('.') {
			let mut str = folder.to_string();
			str.remove(0);
			folder = str;
		}

		let connection_str = format!("{}:{}", self.args.host, "21");
		let mut ftp = FtpStream::connect(connection_str).unwrap();

		let _ = ftp.login(&self.args.user, &self.args.password);

		let file = format!("{}/revision.json", folder);

		let mut recived = ftp.get(&file)?;

		let mut content = String::from("");

		let _ = recived.read_to_string(&mut content);

		let revision = serde_json::from_str::<Revision>(&content)?;

		let _ = ftp.quit();

		Ok(revision)
	}

	fn create_dist(&self) -> Result<PathBuf, anyhow::Error> {

		let args = &self.args;
		let local_repo = args.local.clone();
		let dest = args.destination.clone();
		let verbose = args.verbose;

		let pid = std::process::id();
		let temp_dir = env::temp_dir().to_slash().unwrap().to_string();

		let mut export_path = PathBuf::new();

		env::set_current_dir(&local_repo).unwrap();

		let dest_path = PathBuf::from(dest.clone());

		let dir_name = dest_path.file_name().unwrap();

		let folder_name = dir_name.to_str().unwrap();
		
		let mut deploy_path = PathBuf::new();

		deploy_path.push(temp_dir.clone());
		deploy_path.push(format!("deploy-{}", pid));

		let file_path = deploy_path.join(format!("{}.zip",folder_name));

		if file_path.exists() {

			fs::remove_file(file_path.clone()).unwrap();
		}

		if !deploy_path.exists(){

			fs::create_dir_all(deploy_path.as_path()).unwrap();
		}

		let head_output = Command::new("git")
			.arg("rev-parse")
			.arg("HEAD")
			.output().unwrap();

		let head_ref = String::from_utf8(head_output.stdout).unwrap();

		let head_ref = head_ref.trim_end();

		let cmd = format!("git archive -o {} HEAD", file_path.clone().to_str().unwrap());

		let cmd_output = if cfg!(target_os = "windows") {

			Command::new("powershell")
				.args(["-c", cmd.clone().as_str()])
				.stdout(Stdio::inherit())
				.output().unwrap()
		} else {
			Command::new("bash")
					.args(["-c", cmd.clone().as_str()])
					.stdout(Stdio::inherit())
					.output().unwrap()
		};

		let mut success = cmd_output.status.success();

		if success {

			let zip_file = fs::File::open(file_path.clone()).unwrap();

			let mut archive = ZipArchive::new(zip_file).unwrap();

			if verbose {
				println!();
				println!("Extracting {}", file_path.clone().to_str().unwrap());
				println!();
			}

			for i in 0..archive.len() {

				let mut file = archive.by_index(i).unwrap();

				let outpath = match file.enclosed_name() {
					Some(path) => path.to_owned(),
					None => continue,
				};

				let mut out_path = PathBuf::new();
				out_path.push(deploy_path.clone());
				out_path.push(folder_name);
				out_path.push(outpath.clone());
		
				if (file.name()).ends_with('/') {

					if verbose {
						println!("Creating directory: {}", out_path.clone().to_str().unwrap());
					}

					fs::create_dir_all(&out_path).unwrap();

				} else {
					if let Some(p) = out_path.parent() {
						if !p.exists() {
							
							if verbose {
								println!("Creating file: {}", file.name());
							}
								
							fs::create_dir_all(p).unwrap();
						}
					}
					let mut outfile = fs::File::create(&out_path).unwrap();
					io::copy(&mut file, &mut outfile).unwrap();
				}
			}

			fs::remove_file(file_path).unwrap();

			export_path.push(deploy_path.clone());
			export_path.push(folder_name);

			env::set_current_dir(&local_repo).unwrap();

			let branch_output = Command::new("git")
				.arg("rev-parse")
				.arg("--abbrev-ref")
				.arg("HEAD")
				.output().unwrap();

			success = branch_output.status.success();

			if success {
				
				env::set_current_dir(export_path.clone()).unwrap();

				let branch = String::from_utf8(branch_output.stdout).unwrap();

				let branch = branch.trim_end();

				if verbose {
					println!();
					println!("Creating revision.json");
				}
				
				let revision = Revision::new(head_ref, branch);

				let revision_json = serde_json::to_string(&revision).unwrap();

				let mut revision_path = export_path.clone();

				revision_path.push(deploy_path.clone());
				revision_path.push(folder_name);
				revision_path.push("revision.json");

				let mut revision_file = fs::OpenOptions::new().write(true).create_new(true).open(revision_path).unwrap();

				revision_file.write_all(revision_json.as_bytes()).unwrap();

				println!();
				println!("BRANCH: {}", &branch);
				println!("HEAD: {}", &head_ref);
				println!("SERVER: {}", &head_ref);
				println!("LOCAL: {}", export_path.clone().to_slash().unwrap());
			}
			else {
				return Err(anyhow!("Could not create revision file"));
			}
			
		}
		else {
			return Err(anyhow!("Could not create git archive"));
		}

		Ok(export_path)
	}

	fn create_export(&self) -> Result<PathBuf, anyhow::Error> {

		let args = &self.args;
		let local_repo = args.local.clone();
		let dest = args.destination.clone();
		let verbose = args.verbose;

		let pid = std::process::id();
		let temp_dir = env::temp_dir().to_slash().unwrap().to_string();

		let mut export_path = PathBuf::new();

		let revision_file_server = self.get_revision_file()?;

		env::set_current_dir(&local_repo)?;

		let dest_path = PathBuf::from(dest.clone());

		let dir_name = dest_path.file_name().unwrap();

		let folder_name = dir_name.to_str().unwrap();
		
		let mut deploy_path = PathBuf::new();

		deploy_path.push(temp_dir.clone());
		deploy_path.push(format!("deploy-{}", pid));

		let file_path = deploy_path.join(format!("{}.zip",folder_name));

		if file_path.exists() {

			fs::remove_file(file_path.clone())?;
		}

		if !deploy_path.exists(){

			fs::create_dir_all(deploy_path.as_path()).unwrap();
		}

		let head_output = Command::new("git")
			.arg("rev-parse")
			.arg("HEAD")
			.output().unwrap();

		let head_ref = String::from_utf8(head_output.stdout.clone()).unwrap();

		let head_ref = head_ref.trim_end();
		
		if head_ref != revision_file_server.admin.revision {

			let mut temp_list = PathBuf::new();

			temp_list.push(temp_dir);
			temp_list.push("list.txt");

			let temp_slashed = temp_list.to_slash().unwrap();

			let cmd = format!("git diff --name-only --diff-filter=d {} HEAD > {}", revision_file_server.admin.revision, temp_slashed.clone());

			let cmd_output = if cfg!(target_os = "windows") {

				Command::new("cmd")
					.args(["/C", cmd.clone().as_str()])
					.stdout(Stdio::inherit())
					.output().unwrap()
			} else {
				Command::new("sh")
						.args(["-c", cmd.clone().as_str()])
						.stdout(Stdio::inherit())
						.output().unwrap()
			};

			let mut success = cmd_output.status.success();

			if success && temp_list.exists() {

				let list_data = fs::read_to_string(&temp_list).unwrap();

				let lines = list_data.lines();

				if verbose {
					println!();
					println!("Extracting {}", file_path.clone().to_str().unwrap());
					println!();
				}
				

				for line in lines {

					let mut out_path = PathBuf::new();
					out_path.push(deploy_path.clone());
					out_path.push(folder_name);
					
					let mut file_path = out_path.clone();
					file_path.push(line);

					let parent_dir = file_path.parent().unwrap();
			
					if !&parent_dir.exists() {

						if verbose {
							println!("Creating path: {}", parent_dir.to_string_lossy());
						}

						fs::create_dir_all(parent_dir).unwrap();
					}

					out_path.push(line);

					let mut outfile = fs::File::create(&out_path).unwrap();

					let mut local_file_path = PathBuf::new();
					local_file_path.push(&local_repo);
					local_file_path.push(line);

					let mut file = fs::OpenOptions::new().read(true).open(local_file_path).unwrap();
					io::copy(&mut file, &mut outfile).unwrap();
				}

				if file_path.exists() {

					fs::remove_file(file_path.clone())?;
				}

				export_path.push(deploy_path.clone());
				export_path.push(folder_name);

				env::set_current_dir(&local_repo).unwrap();

				let branch_output = Command::new("git")
					.arg("rev-parse")
					.arg("--abbrev-ref")
					.arg("HEAD")
					.output().unwrap();

				success = branch_output.status.success();

				if success {

					env::set_current_dir(export_path.clone()).unwrap();

					let branch = String::from_utf8(branch_output.stdout).unwrap();

					let branch = branch.trim_end();

					if verbose {
						println!();
						println!("Creating revision.json");
					}

					let revision = Revision::new(head_ref, branch);

					let revision_json = serde_json::to_string(&revision).unwrap();

					let mut revision_path = export_path.clone();

					revision_path.push(deploy_path.clone());
					revision_path.push(folder_name);
					revision_path.push("revision.json");

					if revision_path.exists() {
						fs::remove_file(revision_path.clone()).unwrap();
					}

					let mut revision_file = fs::OpenOptions::new().write(true).create_new(true).open(revision_path).unwrap();

					revision_file.write_all(revision_json.as_bytes()).unwrap();

					println!();
					println!("BRANCH: {}", &branch);
					println!("HEAD: {}", &head_ref);
					println!("SERVER: {}", &revision_file_server.admin.revision);
					println!("LOCAL: {}", export_path.clone().to_slash().unwrap());

				}
			}
			else {
				return Err(anyhow!(format!("ERROR: {:?}", cmd_output)));
			}
		}
		else {
			return Err(anyhow!(format!("ERROR: {:?}", head_output)));
		}

		Ok(export_path)
	}

	fn deploy(&self) -> Result<bool, anyhow::Error> {

		let args = &self.args;
		let mut dest = args.destination.clone();

		let mut stdout = std::io::stdout();

		let dist = args.dist;
		let create = args.create;

		if dest.starts_with('.') {
			dest.remove(0);
		}

		let local_path = if dist || create {
			self.create_dist()?
		}
		else {
			self.create_export()?
		};

		let mut dest_path = PathBuf::new();

		dest_path.push(dest);

		if dist {
			let time_stamp = chrono::offset::Local::now().format("%Y%m%d-%H%M%S").to_string();

			dest_path.push(time_stamp);
		}

		let local_str = local_path.clone().to_string_lossy().to_string();

		let mut count :u64 = 0;
		let mut current: u64 = 0;

		for entry in WalkDir::new(local_path.clone()).into_iter().filter_map(|e| e.ok()) {

			let meta_data = entry.metadata().unwrap();

			if meta_data.is_file() {
				count += 1;
			}
		}

		println!();

		let connection_str = format!("{}:{}", self.args.host, "21");
		let mut ftp = FtpStream::connect(connection_str).unwrap();

		let _ = ftp.login(&self.args.user, &self.args.password);

		for entry in WalkDir::new(local_path.clone()).into_iter().filter_map(|e| e.ok()) {

			let meta_data = entry.metadata().unwrap();

			let dest_export = entry.path().to_path_buf();

			let mut str_dest = String::from(dest_export.to_str().unwrap());

			str_dest = str_dest.replace(local_str.as_str(), dest_path.to_str().unwrap());

			let export_dest = str_dest.trim();

			let mut export_path = PathBuf::new();

			export_path.push(export_dest);

			let str_export = export_path.to_slash().unwrap().to_string();
			
			if meta_data.is_file() {

				let entry_path = entry.path();

				let file_name = entry_path.file_name().unwrap().to_string_lossy().to_string().replace('"', "");

				let file_path = PathBuf::from(str_export);

				let parent_dir = file_path.parent().unwrap().to_slash().unwrap().to_string();

				ftp.mkdir(&parent_dir).unwrap_or_else(|err| {
					println!("{err}");
				});

				if current > 0 {

					let _ = stdout.execute(cursor::MoveUp(3));
					let _ = stdout.execute(terminal::Clear(terminal::ClearType::FromCursorDown));
				}
				
				current += 1;

				let progress = ((current as f64 / count as f64) * 100.0) as i32;

				writeln!(stdout, "Deploying: {progress}%").unwrap();
				writeln!(stdout, "{file_name}").unwrap();
				writeln!(stdout, "{current} / {count}").unwrap();

				let mut file = OpenOptions::new().read(true).open(entry_path).unwrap();

				let _ = ftp.put(&file_path.to_string_lossy(), &mut file);
			}
		}

		let _ = ftp.quit();

		Ok(true)
	}
}

impl <'a> Executor for FtpExport<'a> {

	fn execute(&self) -> Result<bool, anyhow::Error> {
		
		self.git_pull()?;

		self.deploy()
	}
}