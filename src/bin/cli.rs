use std::os::unix::net::UnixStream;

use clap::{Arg, ArgAction, Command, ValueEnum, value_parser};
use color_eyre::eyre::Result;
use discord_mutex::{
    Action, Changer, IpcPayload, Request, Response, VoiceSetting, error::MutexError, get_ipc_path,
};

fn main() -> Result<()> {
    color_eyre::install()?;

    let cli = Command::new("mutex")
        .subcommand_required(true)
        .arg_required_else_help(true)
        .subcommand(
            Command::new("set")
                .about("Set a voice setting, use \"mutex set --help\" for help")
                .arg(
                    Arg::new("setting")
                        .action(ArgAction::Set)
                        .required(true)
                        .help("The setting to set")
                        .value_parser(value_parser!(VoiceSetting)),
                )
                .arg(
                    Arg::new("action")
                        .action(ArgAction::Set)
                        .required(true)
                        .help("What to set the setting to")
                        .value_parser(value_parser!(ActionParse)),
                ),
        );

    let matches = cli.get_matches();
    // This shoudln't panic required is set to true
    let (subcommand, sub_matches) = matches.subcommand().unwrap();
    let req = match subcommand {
        "set" => {
            // This shoudln't panic required is set to true
            let setting = *sub_matches.get_one("setting").unwrap();
            let action = (*sub_matches.get_one::<ActionParse>("action").unwrap()).into();

            let changer = Changer { action, setting };

            Request::Set(changer)
        }
        _ => panic!(), // This shoudln't happen
    };

    let stream_res: Result<UnixStream> =
        UnixStream::connect(get_ipc_path()?).map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused => {
                { MutexError::DeamonNotStarted }.into()
            }
            _ => e.into(),
        });
    let mut stream = stream_res?;

    req.send(&mut stream)?;

    loop {
        let res = Response::read(&mut stream)?;
        match res {
            Response::Done => break,
            Response::NeedsToken => println!("Check your discord and authorize the app"),
            Response::Error(error) => Err(error)?,
        }
    }

    Ok(())
}

#[derive(Clone, Copy, ValueEnum)]
enum ActionParse {
    True,
    False,
    Toggle,
}

impl Into<Action> for ActionParse {
    fn into(self) -> Action {
        match self {
            Self::True => Action::Force(true),
            Self::False => Action::Force(false),
            Self::Toggle => Action::Toggle,
        }
    }
}
