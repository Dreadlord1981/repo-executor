use clap::Parser;

#[derive(Parser, Debug, Clone)]
#[command(author, version, about = "Utility to update product")]
#[clap(disable_help_flag = true)]

pub struct Arguments {

	#[arg(short('h'),long, help="Host")]
	pub host: String,

	#[arg(short, long, help="Username")]
	pub user: String,

	#[arg(short('w'), long, help="Password")]
	pub password: String,

	#[arg(short('d'), long("dest"), help="Upload destination")]
	pub destination: String,

	#[arg(short('c'), long, help="Create export")]
	pub create: bool,

	#[arg(long="dist", help="Add dist folder ( timestamp )")]
	pub dist: bool,

	#[arg(short('l'), long("local"), default_value = "", help="Local rep")]
	pub local: String,

	/// Build list of files
	#[arg(short('t'), long("list_build"))]
	pub list: bool,

	#[arg(short('b'), long, default_value="master", help="Git branch to expor")]
	pub branch: String,

	#[arg(short('f'), long("ftp"), help="Ftp files to server")]
	pub ftp: bool,

	#[arg(short('v'), long("verbose"), help="Verbose output")]
	pub verbose: bool,

	#[arg(short('s'), long("stdprint"), help="Verbose print to stdout")]
	pub stdprint: bool,
	
	#[arg(short('H'), long("help"), help="Print help", action = clap::ArgAction::Help)]
	pub help: Option<bool>,
}