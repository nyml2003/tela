#import <UIKit/UIKit.h>

#import "SafeArea.h"

bool tela_ios_safe_area_for_view(void *view, TelaIosInsets *out) {
    if (view == NULL || out == NULL) {
        return false;
    }

    UIView *uiView = (__bridge UIView *)view;
    UIEdgeInsets insets = uiView.safeAreaInsets;
    out->top = (float)insets.top;
    out->right = (float)insets.right;
    out->bottom = (float)insets.bottom;
    out->left = (float)insets.left;
    return true;
}
