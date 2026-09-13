#ifndef TOOLCHAIN_ID
#define TOOLCHAIN_ID "unset"
#endif
__attribute__((used)) static const char reprobisect_toolchain_id[] = TOOLCHAIN_ID;
int main(void) { return reprobisect_toolchain_id[0] == '\0'; }
