//! SSH recovery belongs on the host, rather than in Cocoa's local saved state.
use objc2_foundation::{
    ns_string, NSArgumentDomain, NSMutableDictionary, NSNumber, NSUserDefaults,
};

pub fn disable_cocoa_restoration() {
    apply(&NSUserDefaults::standardUserDefaults());
}

fn apply(defaults: &NSUserDefaults) {
    // These overrides live only in this process. Preserve other launch options;
    // never change the user's persistent preferences or other editor processes.
    let domain_name = unsafe { NSArgumentDomain };
    let domain =
        NSMutableDictionary::dictionaryWithDictionary(&defaults.volatileDomainForName(domain_name));
    domain.insert(
        ns_string!("ApplePersistenceIgnoreState"),
        &NSNumber::numberWithBool(true),
    );
    domain.insert(
        ns_string!("NSQuitAlwaysKeepsWindows"),
        &NSNumber::numberWithBool(false),
    );
    // SAFETY: domain contains NSString keys and property-list values, preserving
    // the types returned by NSUserDefaults and adding only NSNumber booleans.
    unsafe { defaults.setVolatileDomain_forName(&domain, domain_name) };
}

#[cfg(test)]
mod tests {
    use super::*;
    use objc2::AnyThread;
    use objc2_foundation::{NSDictionary, NSString};

    #[test]
    fn restoration_overrides_are_volatile_and_preserve_arguments() {
        let name = NSString::from_str(&format!("dev.hedit.test.{}", std::process::id()));
        let defaults =
            NSUserDefaults::initWithSuiteName(NSUserDefaults::alloc(), Some(&name)).unwrap();
        let before = defaults.persistentDomainForName(&name);
        apply(&defaults);
        assert!(defaults.boolForKey(ns_string!("ApplePersistenceIgnoreState")));
        let marker = ns_string!("hedit-test-marker");
        let value = NSNumber::numberWithBool(true);
        let values =
            NSDictionary::<NSString, objc2::runtime::AnyObject>::from_slices(&[marker], &[&*value]);
        // SAFETY: NSString keys and NSNumber values are valid preference types.
        unsafe { defaults.setVolatileDomain_forName(&values, NSArgumentDomain) };
        apply(&defaults);
        assert!(defaults.boolForKey(marker));
        assert!(defaults.boolForKey(ns_string!("ApplePersistenceIgnoreState")));
        assert!(!defaults.boolForKey(ns_string!("NSQuitAlwaysKeepsWindows")));
        assert_eq!(before, defaults.persistentDomainForName(&name));
    }
}
