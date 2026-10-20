//! Native filename validation is advisory UI; eml-app owns export admission and publication.
use super::{NativeError, path_from_url};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{MainThreadMarker, MainThreadOnly, Message, define_class, msg_send};
use objc2_app_kit::{NSOpenSavePanelDelegate, NSSavePanel};
use objc2_foundation::{NSObject, NSObjectProtocol, NSString};

pub(super) const SAVE_MESSAGE: &str = "Export metadata only. Existing files will not be replaced.";
pub(super) const OCCUPIED_MESSAGE: &str = "An item already exists with that name. Choose a different name; existing items are never replaced.";
const UNAVAILABLE_MESSAGE: &str =
    "The report destination could not be checked. Choose another local folder or filename.";

define_class!(
    // SAFETY: NSObject has no additional subclass invariants. No custom Drop or
    // owned panel reference is stored; AppKit keeps only a weak delegate link.
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[ivars = ()]
    pub(super) struct ReportNameDelegate;
    // SAFETY: NSObjectProtocol adds no implementation requirements.
    unsafe impl NSObjectProtocol for ReportNameDelegate {}
    // SAFETY: only the documented typed filename callback is implemented.
    unsafe impl NSOpenSavePanelDelegate for ReportNameDelegate {
        #[unsafe(method_id(panel:userEnteredFilename:confirmed:))]
        fn user_filename(
            &self,
            sender: &AnyObject,
            filename: &NSString,
            confirmed: bool,
        ) -> Option<Retained<NSString>> {
            validate_name(sender, filename, confirmed)
        }
    }
);
fn validate_name(
    sender: &AnyObject,
    filename: &NSString,
    confirmed: bool,
) -> Option<Retained<NSString>> {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let panel = sender.downcast_ref::<NSSavePanel>()?;
        if !confirmed {
            return Some(filename.retain());
        }
        match proposed_name(panel) {
            Ok(eml_app::ReportNameAvailability::Unused) => {
                panel.setMessage(Some(&NSString::from_str(SAVE_MESSAGE)));
                Some(filename.retain())
            }
            Ok(eml_app::ReportNameAvailability::Occupied) => {
                panel.setMessage(Some(&NSString::from_str(OCCUPIED_MESSAGE)));
                None
            }
            Err(_) => {
                panel.setMessage(Some(&NSString::from_str(UNAVAILABLE_MESSAGE)));
                None
            }
        }
    }));
    result.unwrap_or_else(|_| {
        if let Some(panel) = sender.downcast_ref::<NSSavePanel>() {
            panel.setMessage(Some(&NSString::from_str(UNAVAILABLE_MESSAGE)));
        }
        None
    })
}

impl ReportNameDelegate {
    pub(super) fn new(main: MainThreadMarker) -> Retained<Self> {
        let allocated = Self::alloc(main).set_ivars(());
        // SAFETY: NSObject init is valid on this allocated main-thread subclass.
        unsafe { msg_send![super(allocated), init] }
    }
}
fn proposed_name(
    panel: &NSSavePanel,
) -> Result<eml_app::ReportNameAvailability, Box<dyn std::error::Error>> {
    // Do not reconstruct from filename/directory: AppKit handles path navigation,
    // normalization and extension presentation. The exact prospective URL was
    // checked against the chosen URL in the native design experiment.
    let url = panel.URL().ok_or(NativeError::InvalidNativePath)?;
    let path = path_from_url(&url)?;
    Ok(eml_app::inspect_report_destination(&path)?)
}
