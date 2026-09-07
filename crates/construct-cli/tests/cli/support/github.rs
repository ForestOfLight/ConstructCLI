pub fn build_mcaddon_bytes() -> Vec<u8> {
    use std::io::Write;

    pub fn manifest(name: &str, uuid: &str, module: &str) -> Vec<u8> {
        format!(
            r#"{{"format_version":2,
                "header":{{"name":"{name}","uuid":"{uuid}","version":[1,2,0]}},
                "modules":[{{"type":"{module}","uuid":"22222222-2222-2222-2222-222222222222","version":[1,0,0]}}]}}"#
        )
        .into_bytes()
    }

    let mut cursor = std::io::Cursor::new(Vec::new());
    let mut zip = zip::ZipWriter::new(&mut cursor);
    let options: zip::write::FileOptions<'_, ()> =
        zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    let bp = manifest(
        "Construct [BP] v1.2.0",
        "8c0c0153-d8b9-482a-889f-aef922b8fe58",
        "data",
    );
    let rp = manifest(
        "Construct [RP] v1.2.0",
        "375ec465-3dc1-429f-8b4c-a337889e1ed4",
        "resources",
    );

    zip.start_file("Construct[BP]/manifest.json", options)
        .unwrap();
    zip.write_all(&bp).unwrap();
    zip.start_file("Construct[BP]/scripts/main.js", options)
        .unwrap();
    zip.write_all(b"// code").unwrap();
    zip.start_file("Construct[BP]/structures/construct.mcstructure", options)
        .unwrap();
    zip.write_all(b"shipped").unwrap();
    zip.start_file("Construct[RP]/manifest.json", options)
        .unwrap();
    zip.write_all(&rp).unwrap();
    zip.finish().unwrap();

    cursor.into_inner()
}

pub fn stub_github(addon: Vec<u8>) -> (String, std::thread::JoinHandle<()>) {
    use std::io::{BufRead, BufReader, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let base = format!("http://127.0.0.1:{port}");
    let release = format!(
        r#"{{"tag_name":"v1.2.0","assets":[{{"name":"Construct-v1.2.0.mcaddon","size":{},"browser_download_url":"{base}/download"}}]}}"#,
        addon.len()
    );

    let handle = std::thread::spawn(move || {
        for _ in 0..2 {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let mut line = String::new();
            BufReader::new(stream.try_clone().unwrap())
                .read_line(&mut line)
                .unwrap();
            let body: Vec<u8> = if line.contains("/download") {
                addon.clone()
            } else {
                release.clone().into_bytes()
            };
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/octet-stream\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(&body);
            let _ = stream.flush();
        }
    });
    (base, handle)
}

pub fn stub_github_wrong_size(
    addon: Vec<u8>,
    claimed_size: u64,
) -> (String, std::thread::JoinHandle<()>) {
    use std::io::{BufRead, BufReader, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let base = format!("http://127.0.0.1:{port}");
    let release = format!(
        r#"{{"tag_name":"v1.2.0","assets":[{{"name":"Construct-v1.2.0.mcaddon","size":{claimed_size},"browser_download_url":"{base}/download"}}]}}"#
    );

    let handle = std::thread::spawn(move || {
        for _ in 0..2 {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let mut line = String::new();
            BufReader::new(stream.try_clone().unwrap())
                .read_line(&mut line)
                .unwrap();
            let body: Vec<u8> = if line.contains("/download") {
                addon.clone()
            } else {
                release.clone().into_bytes()
            };
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/octet-stream\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(&body);
            let _ = stream.flush();
        }
    });
    (base, handle)
}

pub fn stub_github_release(tag: &str) -> (String, std::thread::JoinHandle<()>) {
    use std::io::{BufRead, BufReader, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let base = format!("http://127.0.0.1:{port}");
    let release = format!(r#"{{"tag_name":"{tag}","assets":[]}}"#);

    let handle = std::thread::spawn(move || {
        let Ok((mut stream, _)) = listener.accept() else {
            return;
        };
        let mut line = String::new();
        let _ = BufReader::new(stream.try_clone().unwrap()).read_line(&mut line);
        let body = release.into_bytes();
        let head = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let _ = stream.write_all(head.as_bytes());
        let _ = stream.write_all(&body);
        let _ = stream.flush();
    });
    (base, handle)
}

pub fn stub_github_status(
    status: u16,
    headers: &[(&str, &str)],
    body: &str,
) -> (String, std::thread::JoinHandle<()>) {
    use std::io::{BufRead, BufReader, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let base = format!("http://127.0.0.1:{port}");
    let body = body.as_bytes().to_vec();
    let extra_headers: String = headers
        .iter()
        .map(|(k, v)| format!("{k}: {v}\r\n"))
        .collect();
    let reason = match status {
        403 => "Forbidden",
        404 => "Not Found",
        429 => "Too Many Requests",
        _ => "Error",
    };

    let handle = std::thread::spawn(move || {
        let Ok((mut stream, _)) = listener.accept() else {
            return;
        };
        let mut line = String::new();
        let _ = BufReader::new(stream.try_clone().unwrap()).read_line(&mut line);
        let head = format!(
            "HTTP/1.1 {status} {reason}\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n{extra_headers}\r\n",
            body.len()
        );
        let _ = stream.write_all(head.as_bytes());
        let _ = stream.write_all(&body);
        let _ = stream.flush();
    });
    (base, handle)
}
