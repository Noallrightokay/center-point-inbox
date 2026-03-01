using System.Security.Cryptography;

namespace CenterPointInbox.Shared.Security;

/// <summary>
/// Provides AES-256-GCM encryption and decryption for OAuth provider tokens.
/// Ensures tokens are stored encrypted at rest with authenticated encryption.
/// </summary>
public static class TokenEncryption
{
    private const int NonceByteSize = 12;  // 96 bits, recommended for AES-GCM
    private const int TagByteSize = 16;    // 128 bits, full authentication tag
    private const int KeyByteSize = 32;    // 256 bits for AES-256

    /// <summary>
    /// Encrypts plaintext using AES-256-GCM with a randomly generated nonce.
    /// Output format: [12-byte nonce][16-byte tag][ciphertext], base64-encoded.
    /// </summary>
    /// <param name="plaintext">The plaintext string to encrypt.</param>
    /// <param name="key">Base64-encoded 256-bit encryption key.</param>
    /// <returns>Base64-encoded string containing nonce + tag + ciphertext.</returns>
    public static string Encrypt(string plaintext, string key)
    {
        ArgumentException.ThrowIfNullOrEmpty(plaintext);
        ArgumentException.ThrowIfNullOrEmpty(key);

        var keyBytes = Convert.FromBase64String(key);
        ValidateKeyLength(keyBytes);

        var plaintextBytes = System.Text.Encoding.UTF8.GetBytes(plaintext);
        var nonce = new byte[NonceByteSize];
        RandomNumberGenerator.Fill(nonce);

        var ciphertext = new byte[plaintextBytes.Length];
        var tag = new byte[TagByteSize];

        using var aesGcm = new AesGcm(keyBytes, TagByteSize);
        aesGcm.Encrypt(nonce, plaintextBytes, ciphertext, tag);

        // Combine: [nonce][tag][ciphertext]
        var result = new byte[NonceByteSize + TagByteSize + ciphertext.Length];
        Buffer.BlockCopy(nonce, 0, result, 0, NonceByteSize);
        Buffer.BlockCopy(tag, 0, result, NonceByteSize, TagByteSize);
        Buffer.BlockCopy(ciphertext, 0, result, NonceByteSize + TagByteSize, ciphertext.Length);

        return Convert.ToBase64String(result);
    }

    /// <summary>
    /// Decrypts a base64-encoded AES-256-GCM ciphertext.
    /// Expects input format: [12-byte nonce][16-byte tag][ciphertext], base64-encoded.
    /// </summary>
    /// <param name="ciphertext">Base64-encoded string containing nonce + tag + ciphertext.</param>
    /// <param name="key">Base64-encoded 256-bit encryption key.</param>
    /// <returns>The decrypted plaintext string.</returns>
    public static string Decrypt(string ciphertext, string key)
    {
        ArgumentException.ThrowIfNullOrEmpty(ciphertext);
        ArgumentException.ThrowIfNullOrEmpty(key);

        var keyBytes = Convert.FromBase64String(key);
        ValidateKeyLength(keyBytes);

        var encryptedBytes = Convert.FromBase64String(ciphertext);

        if (encryptedBytes.Length < NonceByteSize + TagByteSize)
        {
            throw new CryptographicException("Encrypted data is too short to contain the required nonce and authentication tag.");
        }

        var nonce = new byte[NonceByteSize];
        var tag = new byte[TagByteSize];
        var ciphertextBytes = new byte[encryptedBytes.Length - NonceByteSize - TagByteSize];

        Buffer.BlockCopy(encryptedBytes, 0, nonce, 0, NonceByteSize);
        Buffer.BlockCopy(encryptedBytes, NonceByteSize, tag, 0, TagByteSize);
        Buffer.BlockCopy(encryptedBytes, NonceByteSize + TagByteSize, ciphertextBytes, 0, ciphertextBytes.Length);

        var plaintextBytes = new byte[ciphertextBytes.Length];

        using var aesGcm = new AesGcm(keyBytes, TagByteSize);
        aesGcm.Decrypt(nonce, ciphertextBytes, tag, plaintextBytes);

        return System.Text.Encoding.UTF8.GetString(plaintextBytes);
    }

    /// <summary>
    /// Generates a new random 256-bit key suitable for AES-256-GCM, returned as a base64 string.
    /// </summary>
    public static string GenerateKey()
    {
        var key = new byte[KeyByteSize];
        RandomNumberGenerator.Fill(key);
        return Convert.ToBase64String(key);
    }

    private static void ValidateKeyLength(byte[] keyBytes)
    {
        if (keyBytes.Length != KeyByteSize)
        {
            throw new ArgumentException(
                $"Encryption key must be exactly {KeyByteSize} bytes (256 bits). Got {keyBytes.Length} bytes.",
                nameof(keyBytes));
        }
    }
}
