use crate::config::{self, Result};
use serde::{Deserialize, Serialize};
use std::{fs, path::Path};
use windows_sys::Win32::{Foundation::GlobalFree, Networking::WinInet::*};

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct ProxySettings {
    flags: u32,
    server: String,
    bypass: String,
    automatic_url: String,
}

#[derive(Serialize, Deserialize)]
struct Recovery {
    previous: ProxySettings,
    applied: ProxySettings,
}

pub fn read() -> Result<ProxySettings> {
    let mut options = [
        INTERNET_PER_CONN_FLAGS,
        INTERNET_PER_CONN_PROXY_SERVER,
        INTERNET_PER_CONN_PROXY_BYPASS,
        INTERNET_PER_CONN_AUTOCONFIG_URL,
    ]
    .map(|dw_option| INTERNET_PER_CONN_OPTIONW {
        dwOption: dw_option,
        ..Default::default()
    });
    let mut list = INTERNET_PER_CONN_OPTION_LISTW {
        dwSize: std::mem::size_of::<INTERNET_PER_CONN_OPTION_LISTW>() as u32,
        dwOptionCount: options.len() as u32,
        pOptions: options.as_mut_ptr(),
        ..Default::default()
    };
    let mut size = list.dwSize;
    // WinINet fills the specified union members and allocates the returned strings.
    if unsafe {
        InternetQueryOptionW(
            std::ptr::null(),
            INTERNET_OPTION_PER_CONNECTION_OPTION,
            std::ptr::from_mut(&mut list).cast(),
            &mut size,
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error().into());
    }
    let mut strings = Vec::new();
    for option in &options[1..] {
        let pointer = unsafe { option.Value.pszValue };
        let mut text = String::new();
        if !pointer.is_null() {
            let mut length = 0;
            unsafe {
                while *pointer.add(length) != 0 {
                    length += 1;
                }
                text = String::from_utf16_lossy(std::slice::from_raw_parts(pointer, length));
                GlobalFree(pointer.cast());
            }
        }
        strings.push(text);
    }
    Ok(ProxySettings {
        flags: unsafe { options[0].Value.dwValue },
        server: strings.remove(0),
        bypass: strings.remove(0),
        automatic_url: strings.remove(0),
    })
}

fn write(settings: &ProxySettings) -> Result<()> {
    let mut server: Vec<u16> = settings.server.encode_utf16().chain(Some(0)).collect();
    let mut bypass: Vec<u16> = settings.bypass.encode_utf16().chain(Some(0)).collect();
    let mut automatic: Vec<u16> = settings
        .automatic_url
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let mut options = [
        INTERNET_PER_CONN_OPTIONW {
            dwOption: INTERNET_PER_CONN_FLAGS,
            Value: INTERNET_PER_CONN_OPTIONW_0 {
                dwValue: settings.flags,
            },
        },
        INTERNET_PER_CONN_OPTIONW {
            dwOption: INTERNET_PER_CONN_PROXY_SERVER,
            Value: INTERNET_PER_CONN_OPTIONW_0 {
                pszValue: server.as_mut_ptr(),
            },
        },
        INTERNET_PER_CONN_OPTIONW {
            dwOption: INTERNET_PER_CONN_PROXY_BYPASS,
            Value: INTERNET_PER_CONN_OPTIONW_0 {
                pszValue: bypass.as_mut_ptr(),
            },
        },
        INTERNET_PER_CONN_OPTIONW {
            dwOption: INTERNET_PER_CONN_AUTOCONFIG_URL,
            Value: INTERNET_PER_CONN_OPTIONW_0 {
                pszValue: automatic.as_mut_ptr(),
            },
        },
    ];
    let list = INTERNET_PER_CONN_OPTION_LISTW {
        dwSize: std::mem::size_of::<INTERNET_PER_CONN_OPTION_LISTW>() as u32,
        dwOptionCount: options.len() as u32,
        pOptions: options.as_mut_ptr(),
        ..Default::default()
    };
    // Buffers are owned locally and remain alive throughout these synchronous calls.
    if unsafe {
        InternetSetOptionW(
            std::ptr::null(),
            INTERNET_OPTION_PER_CONNECTION_OPTION,
            std::ptr::from_ref(&list).cast(),
            list.dwSize,
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error().into());
    }
    for option in [INTERNET_OPTION_SETTINGS_CHANGED, INTERNET_OPTION_REFRESH] {
        if unsafe { InternetSetOptionW(std::ptr::null(), option, std::ptr::null(), 0) } == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
    }
    Ok(())
}

pub fn restore(directory: &Path) -> Result<()> {
    let path = directory.join("system-proxy-recovery.json");
    if !path.exists() {
        return Ok(());
    }
    let recovery: Recovery = serde_json::from_slice(&fs::read(&path)?)?;
    if read()? == recovery.applied {
        write(&recovery.previous)?;
    }
    fs::remove_file(path)?;
    Ok(())
}

pub fn enable(directory: &Path, port: u16) -> Result<()> {
    if port == 0 {
        return Err("系统代理需要有效的 HTTP 或 mixed 监听端口".into());
    }
    let previous = read()?;
    let applied = ProxySettings {
        flags: PROXY_TYPE_DIRECT | PROXY_TYPE_PROXY,
        server: format!("127.0.0.1:{port}"),
        bypass: "<local>;localhost;127.*;[::1]".into(),
        automatic_url: String::new(),
    };
    let recovery = Recovery { previous, applied };
    config::write_atomic(
        &directory.join("system-proxy-recovery.json"),
        &serde_json::to_vec_pretty(&recovery)?,
    )?;
    if let Err(error) = write(&recovery.applied) {
        restore(directory)
            .map_err(|restore| format!("系统代理设置失败：{error}；恢复失败：{restore}"))?;
        return Err(error);
    }
    if read()? != recovery.applied {
        restore(directory)?;
        return Err("Windows 系统代理未达到请求状态".into());
    }
    Ok(())
}

pub fn status() -> Result<String> {
    let settings = read()?;
    if settings.flags & PROXY_TYPE_PROXY == 0 {
        return Ok("关闭".into());
    }
    Ok(settings.server)
}
