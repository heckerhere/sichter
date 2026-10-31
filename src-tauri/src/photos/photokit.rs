//! PhotoKit-Anbindung (macOS). Nur lesende Aufrufe; geschrieben wird allein in
//! die Datei, die fsops vorgibt.

use std::path::Path;
use std::sync::Mutex;
use std::sync::mpsc;

use block2::RcBlock;
use objc2::rc::{Retained, autoreleasepool};
use objc2::runtime::{AnyObject, Bool};
use objc2::{msg_send, sel};
use objc2_foundation::{NSArray, NSError, NSNumber, NSString, NSURL};
use objc2_photos::{
    PHAccessLevel, PHAsset, PHAssetMediaType, PHAssetResource, PHAssetResourceManager, PHAssetResourceRequestOptions, PHAssetResourceType,
    PHAssetSourceType, PHAuthorizationStatus, PHFetchOptions, PHPhotoLibrary,
};

use super::{Access, Asset, ListOptions, PhotoLibrary, Resource, ResourceKind};

pub struct PhotoKit;

fn map_status(s: PHAuthorizationStatus) -> Access {
    match s {
        PHAuthorizationStatus::Authorized => Access::Authorized,
        PHAuthorizationStatus::Limited => Access::Limited,
        PHAuthorizationStatus::Denied => Access::Denied,
        PHAuthorizationStatus::Restricted => Access::Restricted,
        _ => Access::NotDetermined,
    }
}

pub fn access() -> Access {
    // SAFETY: Klassenmethode ohne Vorbedingungen.
    map_status(unsafe { PHPhotoLibrary::authorizationStatusForAccessLevel(PHAccessLevel::ReadWrite) })
}

pub fn request_access() -> Access {
    let current = access();
    if current != Access::NotDetermined {
        return current;
    }
    let (tx, rx) = mpsc::channel();
    let tx = Mutex::new(Some(tx));
    let handler = RcBlock::new(move |status: PHAuthorizationStatus| {
        if let Some(tx) = tx.lock().ok().and_then(|mut t| t.take()) {
            let _ = tx.send(status);
        }
    });
    // SAFETY: Der Block lebt, bis PhotoKit ihn aufgerufen hat (RcBlock wird kopiert).
    unsafe { PHPhotoLibrary::requestAuthorizationForAccessLevel_handler(PHAccessLevel::ReadWrite, &handler) };
    rx.recv().map(map_status).unwrap_or(Access::Denied)
}

fn resource_kind(t: PHAssetResourceType) -> Option<ResourceKind> {
    match t {
        PHAssetResourceType::Photo => Some(ResourceKind::Photo),
        PHAssetResourceType::Video => Some(ResourceKind::Video),
        PHAssetResourceType::AlternatePhoto => Some(ResourceKind::AlternatePhoto),
        PHAssetResourceType::PairedVideo => Some(ResourceKind::PairedVideo),
        // Bearbeitete Fassungen, Bearbeitungsdaten, Audio: bleiben in Fotos.
        _ => None,
    }
}

fn fetch_options(include_hidden: bool) -> Retained<PHFetchOptions> {
    // SAFETY: einfache Setter auf einem frisch angelegten Objekt.
    unsafe {
        let opts = PHFetchOptions::new();
        opts.setIncludeHiddenAssets(include_hidden);
        // Alle Bilder einer Serie, nicht nur das Titelbild: Aussortieren passiert in Sichter.
        opts.setIncludeAllBurstAssets(true);
        // Eigene Mediathek und per iTunes/Finder synchronisierte, keine geteilten Alben anderer.
        opts.setIncludeAssetSourceTypes(PHAssetSourceType::TypeUserLibrary | PHAssetSourceType::TypeiTunesSynced);
        opts
    }
}

/// Dateigröße über die (nicht dokumentierte, aber verbreitete) Eigenschaft
/// `fileSize`. Nur wenn das Objekt sie wirklich hat, sonst `None`.
fn file_size(r: &PHAssetResource) -> Option<i64> {
    // SAFETY: respondsToSelector: ist auf jedem NSObject definiert; valueForKey:
    // wird nur aufgerufen, wenn der Getter existiert (sonst würde KVC eine
    // Ausnahme werfen).
    unsafe {
        let responds: Bool = msg_send![r, respondsToSelector: sel!(fileSize)];
        if !responds.as_bool() {
            return None;
        }
        let value: Option<Retained<AnyObject>> = msg_send![r, valueForKey: &*NSString::from_str("fileSize")];
        let n = value?.downcast::<NSNumber>().ok()?;
        Some(n.longLongValue()).filter(|s| *s > 0)
    }
}

fn resources(asset: &PHAsset) -> Vec<Resource> {
    // SAFETY: lesende Aufrufe auf gültigen PhotoKit-Objekten.
    unsafe {
        let list = PHAssetResource::assetResourcesForAsset(asset);
        let mut out: Vec<Resource> = (0..list.count())
            .filter_map(|i| {
                let r = list.objectAtIndex(i);
                let kind = resource_kind(r.r#type())?;
                Some(Resource { kind, original_name: r.originalFilename().to_string(), size: file_size(&r) })
            })
            .collect();
        let rank = |k: ResourceKind| match k {
            ResourceKind::Photo | ResourceKind::Video => 0,
            ResourceKind::AlternatePhoto => 1,
            ResourceKind::PairedVideo => 2,
        };
        out.sort_by_key(|r| rank(r.kind));
        out
    }
}

impl PhotoLibrary for PhotoKit {
    fn assets(&self, opts: &ListOptions, progress: &dyn Fn(usize, usize)) -> Result<Vec<Asset>, String> {
        if !access().can_read() {
            return Err(crate::i18n::tr("Kein Zugriff auf die Fotos-Mediathek. Bitte in den Systemeinstellungen unter Datenschutz & Sicherheit → Fotos erlauben.", "No access to the Photos library. Please allow it in System Settings → Privacy & Security → Photos.").into());
        }
        autoreleasepool(|_| {
            let options = fetch_options(opts.include_hidden);
            // SAFETY: lesender Fetch mit gültigen Optionen.
            let result = unsafe { PHAsset::fetchAssetsWithOptions(Some(&options)) };
            let count = unsafe { result.count() };
            let mut out = Vec::with_capacity(count);
            progress(0, count);
            for i in 0..count {
                progress(i, count);
                autoreleasepool(|_| {
                    // SAFETY: Index liegt innerhalb von count.
                    let a = unsafe { result.objectAtIndex(i) };
                    let media = unsafe { a.mediaType() };
                    if media != PHAssetMediaType::Image && media != PHAssetMediaType::Video {
                        return;
                    }
                    let created = unsafe { a.creationDate() }.map(|d| d.timeIntervalSince1970().floor() as i64);
                    out.push(Asset {
                        id: unsafe { a.localIdentifier() }.to_string(),
                        created,
                        favorite: unsafe { a.isFavorite() },
                        resources: resources(&a),
                    });
                });
            }
            progress(count, count);
            Ok(out)
        })
    }

    fn export(&self, asset_id: &str, kind: ResourceKind, original_name: &str, to: &Path, allow_network: bool) -> Result<(), String> {
        let path = to.to_str().ok_or(crate::i18n::tr("Zielpfad ist kein gültiges UTF-8", "Target path is not valid UTF-8"))?;
        autoreleasepool(|_| {
            // SAFETY: lesende PhotoKit-Aufrufe; geschrieben wird nur nach `to`,
            // eine Datei, die fsops gerade frei gewählt hat.
            unsafe {
                let ids = NSArray::from_retained_slice(&[NSString::from_str(asset_id)]);
                let found = PHAsset::fetchAssetsWithLocalIdentifiers_options(&ids, Some(&fetch_options(true)));
                if found.count() == 0 {
                    return Err(crate::i18n::tr("Foto ist nicht mehr in der Mediathek", "Photo is no longer in the library").to_string());
                }
                let asset = found.objectAtIndex(0);
                let list = PHAssetResource::assetResourcesForAsset(&asset);
                let candidates: Vec<Retained<PHAssetResource>> =
                    (0..list.count()).map(|i| list.objectAtIndex(i)).filter(|r| resource_kind(r.r#type()) == Some(kind)).collect();
                let resource = candidates
                    .iter()
                    .find(|r| r.originalFilename().to_string() == original_name)
                    .or(candidates.first())
                    .ok_or(crate::i18n::tr("Originaldatei ist nicht mehr in der Mediathek", "Original file is no longer in the library"))?;

                let options = PHAssetResourceRequestOptions::new();
                options.setNetworkAccessAllowed(allow_network);
                let url = NSURL::fileURLWithPath(&NSString::from_str(path));
                let (tx, rx) = mpsc::channel::<Option<String>>();
                let tx = Mutex::new(Some(tx));
                let done = RcBlock::new(move |error: *mut NSError| {
                    // SAFETY: PhotoKit übergibt nil oder ein gültiges NSError.
                    let message = error.as_ref().map(|e| e.localizedDescription().to_string());
                    if let Some(tx) = tx.lock().ok().and_then(|mut t| t.take()) {
                        let _ = tx.send(message);
                    }
                });
                PHAssetResourceManager::defaultManager().writeDataForAssetResource_toFile_options_completionHandler(resource, &url, Some(&options), &done);
                match rx.recv() {
                    Ok(None) => Ok(()),
                    Ok(Some(message)) if !allow_network => Err(crate::trf!("{} (Original liegt evtl. nur in iCloud)", "{} (the original may only be in iCloud)", message)),
                    Ok(Some(message)) => Err(message),
                    Err(_) => Err(crate::i18n::tr("Export wurde abgebrochen", "Export was cancelled").to_string()),
                }
            }
        })
    }
}
