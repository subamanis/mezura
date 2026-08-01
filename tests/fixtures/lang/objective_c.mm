// mezura-expect lines=12 code=8 comments=1 extra=3 classes=1
#import <Foundation/Foundation.h>

@interface Greeter : NSObject
- (void)greet;
@end

@implementation Greeter
- (void)greet {
    NSLog(@"hello");
}
@end
