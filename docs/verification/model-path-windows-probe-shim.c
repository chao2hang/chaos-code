/* wine 8.0 (Debian bookworm) ships bcrypt.dll but not the Windows 10+ split
   bcryptprimitives.dll that every Rust std binary imports for ProcessPrng, so
   no Rust binary can be loaded at all. This shim supplies that one export and
   forwards it to wine's own bcrypt.dll, so randomness still comes from the
   platform rather than from a fixed stand-in. It prints one line per call, so a
   recorded run can show whether the export was reached: the path probe never
   reaches it, the libtest harness calls it once per test to seed its hasher. */
#include <stdio.h>
#include <windows.h>
#include <bcrypt.h>

#ifndef BCRYPT_USE_SYSTEM_PREFERRED_RNG
#define BCRYPT_USE_SYSTEM_PREFERRED_RNG 0x00000002
#endif

__attribute__((dllexport)) int ProcessPrng(unsigned char *buf, unsigned long long len)
{
    fprintf(stderr, "SHIM ProcessPrng called with %llu bytes\n", len);
    if (len > ULONG_MAX)
        return 0;
    return BCryptGenRandom(NULL, buf, (ULONG)len, BCRYPT_USE_SYSTEM_PREFERRED_RNG) == 0;
}
