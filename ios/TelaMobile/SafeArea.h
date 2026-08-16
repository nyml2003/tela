#ifndef TELA_IOS_SAFE_AREA_H
#define TELA_IOS_SAFE_AREA_H

#include <stdbool.h>

typedef struct TelaIosInsets {
    float top;
    float right;
    float bottom;
    float left;
} TelaIosInsets;

bool tela_ios_safe_area_for_view(void *view, TelaIosInsets *out);

#endif
