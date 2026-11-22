# Metody natywne mostu JNI (nazwy muszą zostać nienaruszone – Rust eksportuje Java_pl_szklo_core_NativeEngine_*).
-keep class pl.szklo.core.NativeEngine { *; }
-keepclasseswithmembernames,includedescriptorclasses class * {
    native <methods>;
}
