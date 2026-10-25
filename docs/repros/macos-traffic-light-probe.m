#import <AppKit/AppKit.h>

int main(void) {
  @autoreleasepool {
    [NSApplication sharedApplication];
    [NSApp setActivationPolicy:NSApplicationActivationPolicyProhibited];
    for (NSNumber *inset in @[@18, @14]) {
      NSWindow *window = [[NSWindow alloc]
        initWithContentRect:NSMakeRect(0, 0, 1200, 800)
        styleMask:(NSWindowStyleMaskTitled | NSWindowStyleMaskClosable |
                   NSWindowStyleMaskMiniaturizable | NSWindowStyleMaskResizable |
                   NSWindowStyleMaskFullSizeContentView)
        backing:NSBackingStoreBuffered defer:NO];
      window.releasedWhenClosed = NO;
      window.titlebarAppearsTransparent = YES;
      window.titleVisibility = NSWindowTitleHidden;
      [window.contentView layoutSubtreeIfNeeded];
      NSButton *close = [window standardWindowButton:NSWindowCloseButton];
      NSView *container = close.superview.superview;
      NSRect button = close.frame;
      NSRect rect = container.frame;
      rect.size.height = button.size.height + inset.doubleValue;
      rect.origin.y = window.frame.size.height - rect.size.height;
      container.frame = rect;
      NSRect absolute = [close convertRect:close.bounds toView:nil];
      NSDictionary *result = @{
        @"insetY": inset,
        @"buttonFrameHeight": @(button.size.height),
        @"buttonFrameOriginY": @(button.origin.y),
        @"containerHeight": @(rect.size.height),
        @"centerFromTop": @(window.frame.size.height - NSMidY(absolute)),
      };
      NSData *json = [NSJSONSerialization dataWithJSONObject:result options:NSJSONWritingSortedKeys error:NULL];
      puts([[NSString alloc] initWithData:json encoding:NSUTF8StringEncoding].UTF8String);
      [window close];
    }
  }
  return 0;
}
