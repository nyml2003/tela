// Android 真机开发协议的稳定常量。它们不包含 I/O，供构建、服务和部署用例共享。

/** APK 内 JNI library 目录与 Android 真机 ABI。 */
export const ANDROID_NDK_ABI = 'arm64-v8a';
/** Android Rust 交叉编译使用的标准 Cargo target。 */
export const ANDROID_RUST_TARGET = 'aarch64-linux-android';
/** ADB reverse 与 WSL mirrored localhost 共用的固定开发端口。 */
export const ANDROID_DEV_PORT = 8000;
/** APK 每次启动都严格请求这个移动 Guest index。 */
export const ANDROID_BUNDLE_INDEX_URL = `http://127.0.0.1:${ANDROID_DEV_PORT}/tela-mobile/latest.json`;

/** CC Remote guest 的开发态索引（同一 serve 进程按路径区分通道）。 */
export const ANDROID_CC_BUNDLE_INDEX_URL = `http://127.0.0.1:${ANDROID_DEV_PORT}/tela-cc/latest.json`;
/** debug applicationId 包含 Gradle 的 applicationIdSuffix。 */
export const ANDROID_DEBUG_PACKAGE = 'dev.tela.mobile.dev';
export const ANDROID_DEBUG_ACTIVITY = 'dev.tela.mobile.TelaActivity';
export const ANDROID_DEBUG_COMPONENT = `${ANDROID_DEBUG_PACKAGE}/${ANDROID_DEBUG_ACTIVITY}`;
