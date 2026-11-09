//! Réseau : état des cartes, connectivité, Wi-Fi.

use std::ffi::c_void;
use std::mem::{offset_of, size_of};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::ptr::{addr_of, null_mut};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use windows::core::{GUID, HRESULT, PCWSTR, PWSTR};
use windows::Win32::Foundation::{
    ERROR_ACCESS_DENIED, ERROR_BAD_PROFILE, ERROR_BUFFER_OVERFLOW, ERROR_INVALID_STATE, ERROR_SERVICE_NOT_ACTIVE,
    HANDLE,
};
use windows::Win32::NetworkManagement::IpHelper::{
    FreeMibTable, GetAdaptersAddresses, GetIpForwardTable2, GetNetworkConnectivityHint, GAA_FLAG_INCLUDE_GATEWAYS,
    GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_MULTICAST, IF_TYPE_ETHERNET_CSMACD, IF_TYPE_IEEE80211, IF_TYPE_PROP_VIRTUAL,
    IF_TYPE_SOFTWARE_LOOPBACK, IF_TYPE_TUNNEL, IP_ADAPTER_ADDRESSES_LH, MIB_IPFORWARD_ROW2, MIB_IPFORWARD_TABLE2,
};
use windows::Win32::NetworkManagement::Ndis::IfOperStatusUp;
use windows::Win32::NetworkManagement::WiFi::{
    dot11_BSS_type_infrastructure, dot11_radio_state_on, wlan_connection_mode_profile, wlan_interface_state_connected,
    wlan_intf_opcode_current_connection, wlan_intf_opcode_radio_state, wlan_notification_acm_connection_complete,
    WlanCloseHandle, WlanConnect, WlanDeleteProfile, WlanDisconnect, WlanEnumInterfaces, WlanFreeMemory,
    WlanGetAvailableNetworkList, WlanGetProfile, WlanOpenHandle, WlanQueryInterface, WlanRegisterNotification,
    WlanScan, WlanSetProfile, DOT11_AUTH_ALGORITHM, DOT11_AUTH_ALGO_80211_OPEN, DOT11_AUTH_ALGO_80211_SHARED_KEY,
    DOT11_AUTH_ALGO_OWE, DOT11_AUTH_ALGO_RSNA_PSK, DOT11_AUTH_ALGO_WPA3_SAE, DOT11_AUTH_ALGO_WPA_PSK,
    DOT11_CIPHER_ALGORITHM, DOT11_CIPHER_ALGO_GCMP, DOT11_CIPHER_ALGO_GCMP_256, DOT11_CIPHER_ALGO_TKIP, DOT11_SSID,
    L2_NOTIFICATION_DATA, WLAN_API_VERSION_2_0, WLAN_AVAILABLE_NETWORK, WLAN_AVAILABLE_NETWORK_CONNECTED,
    WLAN_AVAILABLE_NETWORK_HAS_PROFILE, WLAN_AVAILABLE_NETWORK_LIST, WLAN_CONNECTION_ATTRIBUTES,
    WLAN_CONNECTION_NOTIFICATION_DATA, WLAN_CONNECTION_PARAMETERS, WLAN_INTERFACE_INFO, WLAN_INTERFACE_INFO_LIST,
    WLAN_INTF_OPCODE, WLAN_NOTIFICATION_SOURCE_ACM, WLAN_NOTIFICATION_SOURCE_NONE, WLAN_PHY_RADIO_STATE,
    WLAN_RADIO_STATE, WLAN_REASON_CODE_ASSOCIATION_FAILURE, WLAN_REASON_CODE_ASSOCIATION_TIMEOUT,
    WLAN_REASON_CODE_MSMSEC_AUTH_START_TIMEOUT, WLAN_REASON_CODE_MSMSEC_AUTH_SUCCESS_TIMEOUT,
    WLAN_REASON_CODE_MSMSEC_KEY_FORMAT, WLAN_REASON_CODE_MSMSEC_KEY_START_TIMEOUT,
    WLAN_REASON_CODE_MSMSEC_KEY_SUCCESS_TIMEOUT, WLAN_REASON_CODE_MSMSEC_PROFILE_KEYMATERIAL_CHAR,
    WLAN_REASON_CODE_MSMSEC_PROFILE_KEY_LENGTH, WLAN_REASON_CODE_MSMSEC_PROFILE_KEY_UNMAPPED_CHAR,
    WLAN_REASON_CODE_MSMSEC_PROFILE_WRONG_KEYTYPE, WLAN_REASON_CODE_MSMSEC_PSK_MISMATCH_SUSPECTED,
    WLAN_REASON_CODE_NETWORK_NOT_AVAILABLE, WLAN_REASON_CODE_NOT_VISIBLE, WLAN_REASON_CODE_SUCCESS,
    WLAN_REASON_CODE_USER_CANCELLED,
};
use windows::Win32::Networking::NetworkListManager::{
    INetworkListManager, NetworkListManager, NLM_CONNECTIVITY, NLM_CONNECTIVITY_IPV4_INTERNET,
    NLM_CONNECTIVITY_IPV4_LOCALNETWORK, NLM_CONNECTIVITY_IPV4_SUBNET, NLM_CONNECTIVITY_IPV6_INTERNET,
    NLM_CONNECTIVITY_IPV6_LOCALNETWORK, NLM_CONNECTIVITY_IPV6_SUBNET,
};
use windows::Win32::Networking::WinSock::{
    IpDadStatePreferred, NetworkConnectivityLevelHintConstrainedInternetAccess,
    NetworkConnectivityLevelHintInternetAccess, NetworkConnectivityLevelHintLocalAccess,
    NetworkConnectivityLevelHintNone, AF_INET, AF_INET6, AF_UNSPEC, NL_NETWORK_CONNECTIVITY_HINT, SOCKADDR_IN,
    SOCKADDR_IN6, SOCKET_ADDRESS,
};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_MULTITHREADED};

use crate::sys::wide;

/// Exécute `f` sur le pool bloquant de Tauri, avec COM (MTA) initialisé le temps de l'appel :
/// une commande async ne doit pas occuper les threads de l'exécuteur avec des appels lents.
pub async fn off_thread<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    tauri::async_runtime::spawn_blocking(move || {
        let com = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
        let out = f();
        if com {
            unsafe { CoUninitialize() };
        }
        out
    })
    .await
    .ok()
}

/// Chaîne UTF-16 d'un tampon de taille fixe, coupée au premier NUL.
pub fn from_wide(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end]).trim().to_string()
}

// ------------------------------------------------------------------- cartes

#[derive(Serialize)]
pub struct Adapter {
    name: String,
    description: String,
    /// "ethernet", "wifi" ou "other"
    kind: &'static str,
    /// Adresses avec leur préfixe (192.168.1.20/24).
    ipv4: Vec<String>,
    ipv6: Vec<String>,
    gateways: Vec<String>,
    dns: Vec<String>,
    mac: String,
    /// Débit du lien en bit/s.
    speed: Option<u64>,
    /// Porte la route par défaut retenue par Windows.
    primary: bool,
    #[serde(skip)]
    luid: u64,
    #[serde(skip)]
    metrics: (u32, u32),
}

#[derive(Serialize)]
pub struct NetworkInfo {
    /// "internet", "constrained", "local", "none" ou "unknown"
    connectivity: &'static str,
    ssid: Option<String>,
    /// Qualité du signal Wi-Fi (0-100).
    signal: Option<u32>,
    adapters: Vec<Adapter>,
}

#[tauri::command]
pub async fn network_info() -> Option<NetworkInfo> {
    off_thread(|| {
        let adapters = adapters();
        let wifi = if adapters.iter().any(|a| a.kind == "wifi") { current_wifi() } else { None };
        NetworkInfo {
            connectivity: connectivity(),
            signal: wifi.as_ref().map(|w| w.1),
            ssid: wifi.map(|w| w.0),
            adapters,
        }
    })
    .await
}

fn adapters() -> Vec<Adapter> {
    let flags = GAA_FLAG_INCLUDE_GATEWAYS | GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST;
    let mut size = 16 * 1024u32;
    // La liste peut grandir entre deux appels (carte branchée) : quelques essais suffisent.
    for _ in 0..4 {
        // u64 pour l'alignement des structures retournées.
        let mut buf = vec![0u64; size as usize / 8 + 1];
        let first = buf.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH;
        let r = unsafe { GetAdaptersAddresses(AF_UNSPEC.0 as u32, flags, None, Some(first), &mut size) };
        if r == ERROR_BUFFER_OVERFLOW.0 {
            continue;
        }
        if r != 0 {
            return Vec::new();
        }
        let mut list = unsafe { parse_adapters(first) };
        mark_primary(&mut list);
        list.sort_by_key(|a| !a.primary);
        return list;
    }
    Vec::new()
}

/// Parcourt une liste chaînée d'IP Helper (adresses, passerelles, DNS).
unsafe fn walk<T>(mut p: *mut T, mut f: impl FnMut(&T) -> *mut T) {
    while let Some(item) = p.as_ref() {
        p = f(item);
    }
}

/// Cartes de virtualisation (Hyper-V, WSL, VMware...) : sans passerelle, elles ne font que du bruit.
fn looks_virtual(text: &str) -> bool {
    const MARKERS: [&str; 8] = ["virtual", "vmware", "vethernet", "wsl", "loopback", "bluetooth", "npcap", "miniport"];
    let text = text.to_lowercase();
    MARKERS.iter().any(|m| text.contains(m))
}

unsafe fn parse_adapters(first: *mut IP_ADAPTER_ADDRESSES_LH) -> Vec<Adapter> {
    let mut out = Vec::new();
    walk(first, |a| {
        if a.OperStatus == IfOperStatusUp && !matches!(a.IfType, IF_TYPE_SOFTWARE_LOOPBACK | IF_TYPE_TUNNEL) {
            out.extend(adapter(a));
        }
        a.Next
    });
    out
}

unsafe fn adapter(a: &IP_ADAPTER_ADDRESSES_LH) -> Option<Adapter> {
    let text = |p: PWSTR| if p.is_null() { String::new() } else { p.to_string().unwrap_or_default() };
    let (name, description) = (text(a.FriendlyName), text(a.Description));

    let mut gateways = Vec::new();
    walk(a.FirstGatewayAddress, |g| {
        gateways.extend(ip(&g.Address).map(|i| i.to_string()));
        g.Next
    });
    let virtual_card = a.IfType == IF_TYPE_PROP_VIRTUAL || looks_virtual(&description) || looks_virtual(&name);
    if virtual_card && gateways.is_empty() {
        return None;
    }

    let (mut ipv4, mut ipv6) = (Vec::new(), Vec::new());
    walk(a.FirstUnicastAddress, |u| {
        match ip(&u.Address) {
            Some(IpAddr::V4(v4)) => ipv4.push(format!("{v4}/{}", u.OnLinkPrefixLength)),
            Some(IpAddr::V6(v6)) if is_global(&v6) && u.DadState == IpDadStatePreferred => {
                ipv6.push(format!("{v6}/{}", u.OnLinkPrefixLength))
            }
            _ => {}
        }
        u.Next
    });
    let mut dns: Vec<String> = Vec::new();
    walk(a.FirstDnsServerAddress, |d| {
        // fec0:0:0:ffff::1, 2 et 3 : serveurs par défaut historiques, jamais réellement configurés.
        // Une box annonce souvent le même serveur par DHCPv6 et par RA : il apparaît alors deux fois.
        if let Some(server) = ip(&d.Address).filter(|i| !is_site_local(i)).map(|i| i.to_string()) {
            if !dns.contains(&server) {
                dns.push(server);
            }
        }
        d.Next
    });

    let len = (a.PhysicalAddressLength as usize).min(a.PhysicalAddress.len());
    let mac = a.PhysicalAddress[..len].iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(":");
    let speed = [a.ReceiveLinkSpeed, a.TransmitLinkSpeed].into_iter().filter(|&s| s > 0 && s != u64::MAX).max();
    let kind = match a.IfType {
        IF_TYPE_ETHERNET_CSMACD => "ethernet",
        IF_TYPE_IEEE80211 => "wifi",
        _ => "other",
    };
    Some(Adapter {
        name,
        description,
        kind,
        ipv4,
        ipv6,
        gateways,
        dns,
        mac,
        speed,
        primary: false,
        luid: a.Luid.Value,
        metrics: (a.Ipv4Metric, a.Ipv6Metric),
    })
}

fn ip(sa: &SOCKET_ADDRESS) -> Option<IpAddr> {
    let p = sa.lpSockaddr;
    if p.is_null() {
        return None;
    }
    let len = usize::try_from(sa.iSockaddrLength).unwrap_or(0);
    unsafe {
        match (*p).sa_family {
            AF_INET if len >= size_of::<SOCKADDR_IN>() => {
                let s = &*(p as *const SOCKADDR_IN);
                Some(IpAddr::V4(Ipv4Addr::from(s.sin_addr.S_un.S_addr.to_ne_bytes())))
            }
            AF_INET6 if len >= size_of::<SOCKADDR_IN6>() => {
                let s = &*(p as *const SOCKADDR_IN6);
                Some(IpAddr::V6(Ipv6Addr::from(s.sin6_addr.u.Byte)))
            }
            _ => None,
        }
    }
}

fn is_global(v6: &Ipv6Addr) -> bool {
    v6.segments()[0] & 0xe000 == 0x2000
}

fn is_site_local(addr: &IpAddr) -> bool {
    matches!(addr, IpAddr::V6(v6) if v6.segments()[0] & 0xffc0 == 0xfec0)
}

/// La carte principale porte la route par défaut de plus faible métrique effective (route + interface),
/// l'IPv4 passant avant l'IPv6.
fn mark_primary(list: &mut [Adapter]) {
    let best = default_routes()
        .into_iter()
        .filter_map(|(luid, v6, metric)| {
            let i = list.iter().position(|a| a.luid == luid)?;
            let (m4, m6) = list[i].metrics;
            Some(((v6, metric.saturating_add(if v6 { m6 } else { m4 })), i))
        })
        .min_by_key(|&(key, _)| key);
    if let Some((_, i)) = best {
        list[i].primary = true;
    }
}

/// Routes par défaut (0.0.0.0/0 et ::/0) : (LUID de l'interface, IPv6 ?, métrique de la route).
fn default_routes() -> Vec<(u64, bool, u32)> {
    let mut table: *mut MIB_IPFORWARD_TABLE2 = null_mut();
    let mut out = Vec::new();
    unsafe {
        if GetIpForwardTable2(AF_UNSPEC, &mut table).is_err() || table.is_null() {
            return out;
        }
        let count = (*table).NumEntries as usize;
        let rows = std::slice::from_raw_parts(addr_of!((*table).Table) as *const MIB_IPFORWARD_ROW2, count);
        for r in rows.iter().filter(|r| r.DestinationPrefix.PrefixLength == 0) {
            out.push((r.InterfaceLuid.Value, r.DestinationPrefix.Prefix.si_family == AF_INET6, r.Metric));
        }
        FreeMibTable(table as *const c_void);
    }
    out
}

fn connectivity() -> &'static str {
    let levels = [
        (NetworkConnectivityLevelHintInternetAccess, "internet"),
        (NetworkConnectivityLevelHintConstrainedInternetAccess, "constrained"),
        (NetworkConnectivityLevelHintLocalAccess, "local"),
        (NetworkConnectivityLevelHintNone, "none"),
    ];
    let mut hint = NL_NETWORK_CONNECTIVITY_HINT::default();
    if unsafe { GetNetworkConnectivityHint(&mut hint) }.is_ok() {
        if let Some(&(_, level)) = levels.iter().find(|(l, _)| *l == hint.ConnectivityLevel) {
            return level;
        }
    }
    // Niveau inconnu ou masqué : on demande au gestionnaire de listes de réseaux.
    let flags = unsafe {
        CoCreateInstance::<_, INetworkListManager>(&NetworkListManager, None, CLSCTX_ALL)
            .and_then(|nlm| nlm.GetConnectivity())
    };
    let Ok(flags) = flags else { return "unknown" };
    let has = |any: &[NLM_CONNECTIVITY]| any.iter().any(|f| flags.0 & f.0 != 0);
    if has(&[NLM_CONNECTIVITY_IPV4_INTERNET, NLM_CONNECTIVITY_IPV6_INTERNET]) {
        "internet"
    } else if has(&[
        NLM_CONNECTIVITY_IPV4_LOCALNETWORK,
        NLM_CONNECTIVITY_IPV6_LOCALNETWORK,
        NLM_CONNECTIVITY_IPV4_SUBNET,
        NLM_CONNECTIVITY_IPV6_SUBNET,
    ]) {
        "local"
    } else {
        "none"
    }
}

// --------------------------------------------------------------------- Wi-Fi

/// Session wlanapi, fermée à la fin de la portée.
struct Wlan(HANDLE);

/// Mémoire rendue par wlanapi, libérée à la fin de la portée.
struct WlanMem<T>(*mut T);

impl<T> Drop for WlanMem<T> {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { WlanFreeMemory(self.0 as *const c_void) };
        }
    }
}

impl Drop for Wlan {
    fn drop(&mut self) {
        unsafe { WlanCloseHandle(self.0, None) };
    }
}

/// Un réseau tel que vu par une interface, avec ce qu'il faut pour s'y connecter.
struct Seen {
    iface: GUID,
    ssid: Vec<u8>,
    signal: u32,
    secure: bool,
    connected: bool,
    profile: Option<String>,
    auth: DOT11_AUTH_ALGORITHM,
    cipher: DOT11_CIPHER_ALGORITHM,
}

impl Seen {
    fn name(&self) -> String {
        String::from_utf8_lossy(&self.ssid).into_owned()
    }

    /// WPA/WPA2/WPA3 personnel : une clé est à saisir si aucun profil n'existe.
    fn needs_password(&self) -> bool {
        self.secure
            && matches!(self.auth, DOT11_AUTH_ALGO_WPA_PSK | DOT11_AUTH_ALGO_RSNA_PSK | DOT11_AUTH_ALGO_WPA3_SAE)
    }
}

fn ssid_bytes(s: &DOT11_SSID) -> Vec<u8> {
    s.ucSSID[..(s.uSSIDLength as usize).min(s.ucSSID.len())].to_vec()
}

impl Wlan {
    fn open() -> Result<Self, String> {
        let (mut version, mut handle) = (0u32, HANDLE::default());
        match unsafe { WlanOpenHandle(WLAN_API_VERSION_2_0, None, &mut version, &mut handle) } {
            0 => Ok(Self(handle)),
            e => Err(win32_text(e)),
        }
    }

    /// Interfaces Wi-Fi : (GUID, connectée ?).
    fn interfaces(&self) -> Vec<(GUID, bool)> {
        let mut list: *mut WLAN_INTERFACE_INFO_LIST = null_mut();
        if unsafe { WlanEnumInterfaces(self.0, None, &mut list) } != 0 {
            return Vec::new();
        }
        let list = WlanMem(list);
        unsafe {
            let Some(head) = list.0.as_ref() else { return Vec::new() };
            let items = std::slice::from_raw_parts(
                addr_of!((*list.0).InterfaceInfo) as *const WLAN_INTERFACE_INFO,
                head.dwNumberOfItems as usize,
            );
            items.iter().map(|i| (i.InterfaceGuid, i.isState == wlan_interface_state_connected)).collect()
        }
    }

    fn query(&self, iface: &GUID, opcode: WLAN_INTF_OPCODE) -> Option<(WlanMem<c_void>, usize)> {
        let (mut size, mut data) = (0u32, null_mut::<c_void>());
        if unsafe { WlanQueryInterface(self.0, iface, opcode, None, &mut size, &mut data, None) } != 0 {
            return None;
        }
        let data = WlanMem(data);
        (!data.0.is_null()).then_some((data, size as usize))
    }

    /// Radio allumée (logiciel et matériel) ; dans le doute, on la considère allumée.
    fn radio_on(&self, iface: &GUID) -> bool {
        let Some((data, size)) = self.query(iface, wlan_intf_opcode_radio_state) else { return true };
        let header = offset_of!(WLAN_RADIO_STATE, PhyRadioState);
        if size < header {
            return true;
        }
        let capacity = (size_of::<WLAN_RADIO_STATE>() - header) / size_of::<WLAN_PHY_RADIO_STATE>();
        unsafe {
            let state = data.0 as *const WLAN_RADIO_STATE;
            let count = ((*state).dwNumberOfPhys as usize)
                .min((size - header) / size_of::<WLAN_PHY_RADIO_STATE>())
                .min(capacity);
            let phys =
                std::slice::from_raw_parts(addr_of!((*state).PhyRadioState) as *const WLAN_PHY_RADIO_STATE, count);
            phys.is_empty()
                || phys.iter().any(|p| {
                    p.dot11SoftwareRadioState == dot11_radio_state_on
                        && p.dot11HardwareRadioState == dot11_radio_state_on
                })
        }
    }

    /// SSID et qualité du signal de la connexion en cours.
    fn connection(&self, iface: &GUID) -> Option<(String, u32)> {
        let (data, size) = self.query(iface, wlan_intf_opcode_current_connection)?;
        if size < size_of::<WLAN_CONNECTION_ATTRIBUTES>() {
            return None;
        }
        let attrs = unsafe { std::ptr::read_unaligned(data.0 as *const WLAN_CONNECTION_ATTRIBUTES) };
        let assoc = attrs.wlanAssociationAttributes;
        let ssid = String::from_utf8_lossy(&ssid_bytes(&assoc.dot11Ssid)).into_owned();
        Some((ssid, assoc.wlanSignalQuality.min(100)))
    }

    fn networks(&self, iface: &GUID) -> Vec<Seen> {
        let mut list: *mut WLAN_AVAILABLE_NETWORK_LIST = null_mut();
        if unsafe { WlanGetAvailableNetworkList(self.0, iface, 0, None, &mut list) } != 0 {
            return Vec::new();
        }
        let list = WlanMem(list);
        let items = unsafe {
            let Some(head) = list.0.as_ref() else { return Vec::new() };
            std::slice::from_raw_parts(
                addr_of!((*list.0).Network) as *const WLAN_AVAILABLE_NETWORK,
                head.dwNumberOfItems as usize,
            )
        };
        items
            .iter()
            .filter(|n| n.dot11BssType == dot11_BSS_type_infrastructure && n.dot11Ssid.uSSIDLength > 0)
            .map(|n| Seen {
                iface: *iface,
                ssid: ssid_bytes(&n.dot11Ssid),
                signal: n.wlanSignalQuality.min(100),
                secure: n.bSecurityEnabled.as_bool(),
                connected: n.dwFlags & WLAN_AVAILABLE_NETWORK_CONNECTED != 0,
                profile: Some(from_wide(&n.strProfileName))
                    .filter(|p| n.dwFlags & WLAN_AVAILABLE_NETWORK_HAS_PROFILE != 0 && !p.is_empty()),
                auth: n.dot11DefaultAuthAlgorithm,
                cipher: n.dot11DefaultCipherAlgorithm,
            })
            .collect()
    }

    fn set_profile(&self, iface: &GUID, xml: &str) -> Result<(), String> {
        let xml = wide(xml);
        let mut reason = 0u32;
        let r =
            unsafe { WlanSetProfile(self.0, iface, 0, PCWSTR(xml.as_ptr()), PCWSTR::null(), true, None, &mut reason) };
        match r {
            0 => Ok(()),
            e if e == ERROR_BAD_PROFILE.0 && reason != 0 => Err(reason_text(reason)),
            e => Err(win32_text(e)),
        }
    }

    /// XML d'un profil enregistré (clé chiffrée, telle que Windows la stocke), pour le remettre tel quel.
    fn profile(&self, iface: &GUID, name: &str) -> Option<String> {
        let name = wide(name);
        let mut xml = PWSTR::null();
        if unsafe { WlanGetProfile(self.0, iface, PCWSTR(name.as_ptr()), None, &mut xml, None, None) } != 0 {
            return None;
        }
        let xml = WlanMem(xml.0);
        (!xml.0.is_null()).then(|| unsafe { PWSTR(xml.0).to_string() }.ok()).flatten()
    }

    fn delete_profile(&self, iface: &GUID, name: &str) {
        let name = wide(name);
        unsafe { WlanDeleteProfile(self.0, iface, PCWSTR(name.as_ptr()), None) };
    }

    /// Lance la connexion puis attend son issue (la demande seule ne dit rien du mot de passe).
    fn connect(&self, net: &Seen, profile: &str) -> Result<(), String> {
        let (tx, rx) = mpsc::channel();
        *WAITER.lock().unwrap_or_else(|e| e.into_inner()) = Some(tx);
        let watching = unsafe {
            WlanRegisterNotification(
                self.0,
                WLAN_NOTIFICATION_SOURCE_ACM,
                true,
                Some(on_notification),
                None,
                None,
                None,
            )
        } == 0;

        let name = wide(profile);
        let params = WLAN_CONNECTION_PARAMETERS {
            wlanConnectionMode: wlan_connection_mode_profile,
            strProfile: PCWSTR(name.as_ptr()),
            pDot11Ssid: null_mut(),
            pDesiredBssidList: null_mut(),
            dot11BssType: dot11_BSS_type_infrastructure,
            dwFlags: 0,
        };
        let outcome = match unsafe { WlanConnect(self.0, &net.iface, &params, None) } {
            0 if watching => wait_connection(&rx, &net.iface, &net.ssid),
            0 => Ok(()),
            e => Err(win32_text(e)),
        };

        if watching {
            unsafe { WlanRegisterNotification(self.0, WLAN_NOTIFICATION_SOURCE_NONE, true, None, None, None, None) };
        }
        *WAITER.lock().unwrap_or_else(|e| e.into_inner()) = None;
        outcome
    }
}

/// Fin de tentative de connexion : (interface, SSID, code de raison).
type Outcome = (GUID, Vec<u8>, u32);

/// Destinataire des notifications pendant une connexion (une seule à la fois, voir CONNECTING).
static WAITER: Mutex<Option<Sender<Outcome>>> = Mutex::new(None);
static CONNECTING: Mutex<()> = Mutex::new(());

const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

unsafe extern "system" fn on_notification(data: *mut L2_NOTIFICATION_DATA, _context: *mut c_void) {
    let Some(d) = data.as_ref() else { return };
    if d.NotificationSource != WLAN_NOTIFICATION_SOURCE_ACM
        || d.NotificationCode != wlan_notification_acm_connection_complete.0 as u32
        || d.pData.is_null()
        || (d.dwDataSize as usize) < offset_of!(WLAN_CONNECTION_NOTIFICATION_DATA, strProfileXml)
    {
        return;
    }
    let n = d.pData as *const WLAN_CONNECTION_NOTIFICATION_DATA;
    let outcome = (d.InterfaceGuid, ssid_bytes(&*addr_of!((*n).dot11Ssid)), (*n).wlanReasonCode);
    if let Ok(waiter) = WAITER.lock() {
        if let Some(tx) = waiter.as_ref() {
            let _ = tx.send(outcome);
        }
    }
}

fn wait_connection(rx: &Receiver<Outcome>, iface: &GUID, ssid: &[u8]) -> Result<(), String> {
    let deadline = Instant::now() + CONNECT_TIMEOUT;
    loop {
        match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok((g, s, reason)) if g == *iface && s == ssid => {
                return if reason == WLAN_REASON_CODE_SUCCESS { Ok(()) } else { Err(reason_text(reason)) };
            }
            Ok(_) => {}
            Err(_) => return Err("Le réseau ne répond pas.".into()),
        }
    }
}

/// Fusionne les réseaux vus par toutes les interfaces : un seul par SSID, le signal le plus fort.
fn merge(mut seen: Vec<Seen>) -> Vec<Seen> {
    seen.sort_by_key(|n| std::cmp::Reverse(n.signal));
    let mut out: Vec<Seen> = Vec::new();
    for n in seen {
        match out.iter_mut().find(|o| o.ssid == n.ssid) {
            Some(o) => {
                o.connected |= n.connected;
                if o.profile.is_none() {
                    o.profile = n.profile;
                }
            }
            None => out.push(n),
        }
    }
    out.sort_by(|a, b| b.connected.cmp(&a.connected).then(b.signal.cmp(&a.signal)));
    out
}

fn current_wifi() -> Option<(String, u32)> {
    let wlan = Wlan::open().ok()?;
    wlan.interfaces().iter().filter(|(_, connected)| *connected).find_map(|(g, _)| wlan.connection(g))
}

#[derive(Serialize)]
pub struct WifiNetwork {
    ssid: String,
    /// Qualité du signal (0-100).
    signal: u32,
    secure: bool,
    connected: bool,
    /// Un profil Windows existe : connexion sans mot de passe.
    saved: bool,
    /// Une clé est à saisir si le réseau n'est pas enregistré.
    password: bool,
}

#[derive(Serialize, Default)]
pub struct WifiState {
    interface: bool,
    radio: bool,
    networks: Vec<WifiNetwork>,
}

#[tauri::command]
pub async fn wifi_networks(scan: bool) -> WifiState {
    off_thread(move || wifi_state(scan)).await.unwrap_or_default()
}

fn wifi_state(scan: bool) -> WifiState {
    let Ok(wlan) = Wlan::open() else { return WifiState::default() };
    let ifaces = wlan.interfaces();
    if ifaces.is_empty() {
        return WifiState::default();
    }
    let radio = ifaces.iter().any(|(g, _)| wlan.radio_on(g));
    if !radio {
        return WifiState { interface: true, radio, networks: Vec::new() };
    }
    if scan && scan_allowed() {
        // Asynchrone : les résultats arrivent dans la liste en quelques secondes.
        for (g, _) in &ifaces {
            unsafe { WlanScan(wlan.0, g, None, None, None) };
        }
    }
    let networks = merge(ifaces.iter().flat_map(|(g, _)| wlan.networks(g)).collect())
        .into_iter()
        .take(40)
        .map(|n| WifiNetwork {
            ssid: n.name(),
            signal: n.signal,
            secure: n.secure,
            connected: n.connected,
            saved: n.profile.is_some(),
            password: n.needs_password(),
        })
        .collect();
    WifiState { interface: true, radio, networks }
}

/// Un balayage actif fait quitter son canal à la carte quelques centaines de ms (pics de latence en jeu ou en
/// appel) : un au plus par minute, et aucun pendant un plein écran. Entre deux, la liste du système suffit.
fn scan_allowed() -> bool {
    static LAST: Mutex<Option<Instant>> = Mutex::new(None);
    if crate::appbar::fullscreen_anywhere() {
        return false;
    }
    let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
    if last.is_some_and(|t| t.elapsed() < SCAN_INTERVAL) {
        return false;
    }
    *last = Some(Instant::now());
    true
}

const SCAN_INTERVAL: Duration = Duration::from_secs(60);

#[tauri::command]
pub async fn wifi_connect(ssid: String, password: Option<String>) -> Result<(), String> {
    off_thread(move || connect(&ssid, password.as_deref().filter(|p| !p.is_empty())))
        .await
        .unwrap_or_else(|| Err("Connexion interrompue.".into()))
}

fn connect(ssid: &str, password: Option<&str>) -> Result<(), String> {
    let _one_at_a_time = CONNECTING.lock().unwrap_or_else(|e| e.into_inner());
    let wlan = Wlan::open()?;
    let seen = wlan.interfaces().iter().flat_map(|(g, _)| wlan.networks(g)).collect();
    let net = merge(seen).into_iter().find(|n| n.name() == ssid).ok_or("Ce réseau n'est plus à portée.")?;

    // `previous` : profil de même nom écrasé par le nouveau (mot de passe ressaisi pour un réseau enregistré).
    let (profile, written, previous) = match (&net.profile, password) {
        (Some(existing), None) => (existing.clone(), false, None),
        _ => {
            let xml = profile_xml(&net, ssid, password)?;
            let previous = wlan.profile(&net.iface, ssid);
            wlan.set_profile(&net.iface, &xml)?;
            (ssid.to_string(), true, previous)
        }
    };
    let outcome = wlan.connect(&net, &profile);
    // Comme Windows : un profil n'est gardé que si la connexion a réussi. Celui qu'il remplaçait revient.
    if outcome.is_err() && written {
        match previous {
            Some(xml) if wlan.set_profile(&net.iface, &xml).is_ok() => {}
            _ => wlan.delete_profile(&net.iface, &profile),
        }
    }
    outcome
}

#[tauri::command]
pub async fn wifi_disconnect() -> Result<(), String> {
    off_thread(|| {
        let wlan = Wlan::open()?;
        for (g, _) in wlan.interfaces().iter().filter(|(_, connected)| *connected) {
            match unsafe { WlanDisconnect(wlan.0, g, None) } {
                0 => {}
                e => return Err(win32_text(e)),
            }
        }
        Ok(())
    })
    .await
    .unwrap_or_else(|| Err("Déconnexion interrompue.".into()))
}

fn profile_xml(net: &Seen, name: &str, password: Option<&str>) -> Result<String, String> {
    let (auth, encryption) = match net.auth {
        _ if !net.secure => ("open", "none"),
        DOT11_AUTH_ALGO_OWE => ("OWE", "AES"),
        DOT11_AUTH_ALGO_WPA3_SAE => ("WPA3SAE", "AES"),
        DOT11_AUTH_ALGO_RSNA_PSK => ("WPA2PSK", cipher(net.cipher)),
        DOT11_AUTH_ALGO_WPA_PSK => ("WPAPSK", cipher(net.cipher)),
        DOT11_AUTH_ALGO_80211_OPEN | DOT11_AUTH_ALGO_80211_SHARED_KEY => {
            return Err("Les réseaux WEP ne sont pas pris en charge.".into())
        }
        _ => return Err("Réseau d'entreprise : connecte-toi depuis les paramètres Wi-Fi de Windows.".into()),
    };
    let shared = if net.needs_password() {
        let (kind, material) = key_material(password.ok_or("Mot de passe requis.")?)?;
        format!(
            "<sharedKey><keyType>{kind}</keyType><protected>false</protected>\
             <keyMaterial>{}</keyMaterial></sharedKey>",
            xml_escape(material)
        )
    } else {
        String::new()
    };
    let hex: String = net.ssid.iter().map(|b| format!("{b:02X}")).collect();
    let name = xml_escape(name);
    Ok(format!(
        r#"<?xml version="1.0"?>
<WLANProfile xmlns="http://www.microsoft.com/networking/WLAN/profile/v1">
<name>{name}</name>
<SSIDConfig><SSID><hex>{hex}</hex><name>{name}</name></SSID></SSIDConfig>
<connectionType>ESS</connectionType>
<connectionMode>auto</connectionMode>
<MSM><security>
<authEncryption><authentication>{auth}</authentication><encryption>{encryption}</encryption><useOneX>false</useOneX></authEncryption>
{shared}
</security></MSM>
</WLANProfile>"#
    ))
}

fn cipher(c: DOT11_CIPHER_ALGORITHM) -> &'static str {
    match c {
        DOT11_CIPHER_ALGO_TKIP => "TKIP",
        DOT11_CIPHER_ALGO_GCMP => "GCMP",
        DOT11_CIPHER_ALGO_GCMP_256 => "GCMP256",
        _ => "AES",
    }
}

/// Clé WPA : phrase de 8 à 63 caractères ASCII imprimables, ou clé brute de 64 chiffres hexadécimaux.
fn key_material(p: &str) -> Result<(&'static str, &str), String> {
    if p.len() == 64 && p.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Ok(("networkKey", p));
    }
    if !(8..=63).contains(&p.chars().count()) {
        return Err("Le mot de passe doit contenir entre 8 et 63 caractères.".into());
    }
    if !p.chars().all(|c| (' '..='~').contains(&c)) {
        return Err("Le mot de passe contient des caractères non pris en charge par le Wi-Fi.".into());
    }
    Ok(("passPhrase", p))
}

fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            // Caractères de contrôle interdits en XML (un SSID peut en contenir).
            c if c < ' ' => out.push('?'),
            c => out.push(c),
        }
    }
    out
}

fn reason_text(code: u32) -> String {
    let known = match code {
        WLAN_REASON_CODE_MSMSEC_PSK_MISMATCH_SUSPECTED
        | WLAN_REASON_CODE_MSMSEC_KEY_START_TIMEOUT
        | WLAN_REASON_CODE_MSMSEC_KEY_SUCCESS_TIMEOUT
        | WLAN_REASON_CODE_MSMSEC_AUTH_START_TIMEOUT
        | WLAN_REASON_CODE_MSMSEC_AUTH_SUCCESS_TIMEOUT => "Mot de passe incorrect.",
        WLAN_REASON_CODE_MSMSEC_PROFILE_KEY_LENGTH
        | WLAN_REASON_CODE_MSMSEC_PROFILE_KEYMATERIAL_CHAR
        | WLAN_REASON_CODE_MSMSEC_PROFILE_KEY_UNMAPPED_CHAR
        | WLAN_REASON_CODE_MSMSEC_PROFILE_WRONG_KEYTYPE
        | WLAN_REASON_CODE_MSMSEC_KEY_FORMAT => "Ce mot de passe n'a pas un format valide pour ce réseau.",
        WLAN_REASON_CODE_NETWORK_NOT_AVAILABLE | WLAN_REASON_CODE_NOT_VISIBLE => "Ce réseau n'est plus à portée.",
        WLAN_REASON_CODE_ASSOCIATION_FAILURE | WLAN_REASON_CODE_ASSOCIATION_TIMEOUT => {
            "Le point d'accès n'a pas accepté la connexion."
        }
        WLAN_REASON_CODE_USER_CANCELLED => "Connexion annulée.",
        _ => "",
    };
    if !known.is_empty() {
        return known.into();
    }
    // La liaison du crate prend un &[u16] alors que la fonction écrit dans le tampon : on la redéclare.
    windows_core::link!("wlanapi.dll" "system" fn WlanReasonCodeToString(
        code: u32, size: u32, buffer: PWSTR, reserved: *const c_void
    ) -> u32);
    let mut buf = [0u16; 512];
    let r = unsafe { WlanReasonCodeToString(code, buf.len() as u32, PWSTR(buf.as_mut_ptr()), std::ptr::null()) };
    let text = if r == 0 { from_wide(&buf) } else { String::new() };
    if text.is_empty() {
        format!("Connexion impossible (code {code}).")
    } else {
        text
    }
}

fn win32_text(code: u32) -> String {
    match code {
        c if c == ERROR_SERVICE_NOT_ACTIVE.0 => "Le service Wi-Fi de Windows (WLAN AutoConfig) est arrêté.".into(),
        c if c == ERROR_ACCESS_DENIED.0 => "Windows a refusé l'opération.".into(),
        c if c == ERROR_INVALID_STATE.0 => "La carte Wi-Fi n'est pas prête (radio coupée ?).".into(),
        c => {
            let text = HRESULT::from_win32(c).message();
            let text = text.trim();
            if text.is_empty() {
                format!("Erreur Wi-Fi {c}.")
            } else {
                text.to_string()
            }
        }
    }
}
