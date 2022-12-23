use std::{net::TcpStream, io::{Write, BufWriter, BufReader, Read}, str};

use haproxy_api::{Action, Core, Txn };
use mlua::prelude::*;
use regex::*;

#[mlua::lua_module]
fn haproxy_simple_module(lua: &Lua) -> LuaResult<bool> {
    let core = Core::new(lua)?;

    core.register_action("sip_forward", &[Action::TcpReq, Action::TcpRes], 0, |_lua, txn: Txn| {

        let sip = txn.get_var::<String>("txn.sip").unwrap();
        let src_port = txn.get_var::<String>("txn.sipRespPort").unwrap();

        println!("{}", sip);


        // Compile regexes to extract data
        let re_reg_header = Regex::new(r"REGISTER *sip:([\w.]+):?(\d+)?").unwrap();
        let re_from_header = Regex::new(r"From: *<sip:(.+)@([\w\.]+):(\d+)").unwrap();
        let re_to_header = Regex::new(r"To: *<sip:(.+)@([\w\.]+):(\d+)").unwrap();
        let re_via_header = Regex::new(r"Via: *(\S+) *([\d\.]+):(\d+)").unwrap();
        let re_cont_header = Regex::new(r"Contact: *<sip:(.+)@([\w\.]+):(\d+)").unwrap();
        let re_via_recv_header = Regex::new(r"Via: *(\S+) *.+received=.*;(\S+)").unwrap();
        let re_from_recv_header = Regex::new(r"From: *<sip:(.+)@([\w\.]+)").unwrap();
        let re_to_recv_header = Regex::new(r"To: *<sip:(.+)@([\w\.]+)").unwrap();

        // Extract SIP header data
        let mut register_data = {
            let captures = re_reg_header.captures(&sip).unwrap();
            (captures.get(1).unwrap().as_str().to_owned(),
                captures.get(2).map_or_else(|| "5060".to_owned(), |s| s.as_str().to_owned()))
        };

        let mut via_data = {
            let captures = re_via_header.captures(&sip).unwrap();
            (captures.get(1).unwrap().as_str().to_owned(), captures.get(2).unwrap().as_str().to_owned(),
                captures.get(3).unwrap().as_str().to_owned())
        };

        let mut from_data = {
            let captures = re_from_header.captures(&sip).unwrap();
            (captures.get(1).unwrap().as_str().to_owned(), captures.get(2).unwrap().as_str().to_owned(),
                captures.get(3).unwrap().as_str().to_owned())
        };

        let mut to_data = {
            let captures = re_to_header.captures(&sip).unwrap();
            (captures.get(1).unwrap().as_str().to_owned(), captures.get(2).unwrap().as_str().to_owned(),
                captures.get(3).unwrap().as_str().to_owned())
        };

        let mut contact_data = {
            let captures = re_cont_header.captures(&sip).unwrap();
            (captures.get(1).unwrap().as_str().to_owned(), captures.get(2).unwrap().as_str().to_owned(),
                captures.get(3).unwrap().as_str().to_owned())
        };

        println!("From: {:#?}\nTo: {:#?}\nVia: {:#?}\nContact: {:#?}", from_data, to_data, via_data, contact_data);

        // Save original data
        let orig_via_data = via_data.clone();
        let orig_from_data = from_data.clone();
        let orig_to_data = to_data.clone();
        let orig_contact_data = contact_data.clone();
        let orig_register_data = register_data.clone();

        // Replace host data with docker data
        register_data.0 = "172.17.0.2".to_owned();
        register_data.1 = "5060".to_owned();

        from_data.1 = "172.17.0.2".to_owned();
        from_data.2 = "5060".to_owned();

        to_data.1 = "172.17.0.2".to_owned();
        to_data.2 = "5060".to_owned();

        let stream = TcpStream::connect("172.17.0.2:5060").unwrap();
        let local_port = stream.local_addr().unwrap().port();

        via_data.1 = "172.17.0.1".to_owned();
        via_data.2 = format!("{}", local_port);

        contact_data.1 = "172.17.0.1".to_owned();
        contact_data.2 = format!("{}", local_port);


        // Alter the header with the new data
        let reg_repl = re_reg_header.replace(&sip, format!("REGISTER sip:{}:{}", register_data.0, register_data.1)).to_string();
        let from_repl = re_from_header.replace(&reg_repl, format!("From: <sip:{}@{}:{}", from_data.0, from_data.1, from_data.2)).to_string();
        let to_repl = re_to_header.replace(&from_repl, format!("To: <sip:{}@{}:{}", to_data.0, to_data.1, to_data.2)).to_string();
        let via_repl = re_via_header.replace(&to_repl, format!("Via: {} {}:{}", via_data.0, via_data.1, via_data.2)).to_string();
        let con_repl = re_cont_header.replace(&via_repl, format!("Contact: <sip:{}@{}:{}", contact_data.0, contact_data.1, contact_data.2)).to_string();

        println!("{}\n###", con_repl);

        // Get a response from Asterisk
        let mut writer = BufWriter::new(stream.try_clone().unwrap());
        let mut reader = BufReader::new(stream);
        writer.write(con_repl.as_bytes()).unwrap();
        writer.flush().unwrap();

        let mut resp_buf = [0_u8; 1024];
        reader.read(&mut resp_buf).unwrap();

        let resp = str::from_utf8(&resp_buf).unwrap().to_owned();

        let via_recv_data = {
            let captures = re_via_recv_header.captures(&resp).unwrap();
            (captures.get(1).unwrap().as_str().to_owned(), captures.get(2).unwrap().as_str().to_owned())
        };

        println!("{}", resp);

        let via_recv_resp = format!("{}:{};rport={};received={}", orig_to_data.1, orig_via_data.2, src_port, orig_to_data.1);

        // Alter the header with the original data
        let from_repl = re_from_recv_header.replace(&resp, format!("From: <sip:{}@{}", orig_from_data.0, orig_from_data.1)).to_string();
        let to_repl = re_to_recv_header.replace(&from_repl, format!("To: <sip:{}@{}", orig_to_data.0, orig_to_data.1)).to_string();
        let via_repl = re_via_header.replace(&to_repl, format!("Via: {} {}:{}", orig_via_data.0, orig_via_data.1, orig_via_data.2)).to_string();
        let via_recv_repl = re_via_recv_header.replace(&via_repl, format!("Via: {} {};{}", via_recv_data.0, via_recv_resp, via_recv_data.1)).to_string();

        println!("{}", via_recv_repl);

        let stream = TcpStream::connect(format!("{}:{}", orig_via_data.1, orig_via_data.2)).unwrap();
        let mut writer = BufWriter::new(stream.try_clone().unwrap());
        writer.write(via_recv_repl.as_bytes()).unwrap();
        writer.flush().unwrap();


        Ok(())
    })?;

    Ok(true)
}