using System.Net.Http.Headers;
using System.Net.Http.Json;
using System.Security.Authentication;
using System.Text.Json;
using System.Text.Json.Serialization;
using CenterPointInbox.AuthService.Configuration;
using CenterPointInbox.AuthService.Models;
using Microsoft.Extensions.Logging;
using Microsoft.Extensions.Options;

namespace CenterPointInbox.AuthService.Services;

/// <summary>
/// Implementation of IOAuthService that exchanges authorization codes and retrieves
/// user profiles from Google and Microsoft OAuth providers via HTTP.
/// </summary>
public class OAuthService : IOAuthService
{
    private readonly AuthSettings _authSettings;
    private readonly IHttpClientFactory _httpClientFactory;
    private readonly ILogger<OAuthService> _logger;

    private static readonly JsonSerializerOptions JsonOptions = new()
    {
        PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower,
        DefaultIgnoreCondition = JsonIgnoreCondition.WhenWritingNull
    };

    public OAuthService(
        IOptions<AuthSettings> authSettings,
        IHttpClientFactory httpClientFactory,
        ILogger<OAuthService> logger)
    {
        _authSettings = authSettings.Value;
        _httpClientFactory = httpClientFactory;
        _logger = logger;
    }

    /// <inheritdoc />
    public async Task<OAuthTokenResult> ExchangeGoogleCodeAsync(string code, string redirectUri)
    {
        ArgumentException.ThrowIfNullOrEmpty(code);
        ArgumentException.ThrowIfNullOrEmpty(redirectUri);

        _logger.LogDebug("Exchanging Google authorization code for tokens");

        var client = _httpClientFactory.CreateClient("OAuth");

        var requestBody = new FormUrlEncodedContent(new Dictionary<string, string>
        {
            ["code"] = code,
            ["client_id"] = _authSettings.Google.ClientId,
            ["client_secret"] = _authSettings.Google.ClientSecret,
            ["redirect_uri"] = redirectUri,
            ["grant_type"] = "authorization_code"
        });

        var response = await client.PostAsync(_authSettings.Google.TokenEndpoint, requestBody);

        if (!response.IsSuccessStatusCode)
        {
            var errorBody = await response.Content.ReadAsStringAsync();
            _logger.LogError(
                "Google token exchange failed with status {StatusCode}: {ErrorBody}",
                response.StatusCode, errorBody);
            throw new AuthenticationException("Failed to exchange Google authorization code for tokens.");
        }

        var tokenResponse = await response.Content.ReadFromJsonAsync<GoogleTokenResponse>(JsonOptions);

        if (tokenResponse is null || string.IsNullOrEmpty(tokenResponse.AccessToken))
        {
            throw new AuthenticationException("Google token response was empty or malformed.");
        }

        _logger.LogInformation("Successfully exchanged Google authorization code for tokens");

        return new OAuthTokenResult
        {
            AccessToken = tokenResponse.AccessToken,
            RefreshToken = tokenResponse.RefreshToken,
            IdToken = tokenResponse.IdToken,
            ExpiresInSeconds = tokenResponse.ExpiresIn,
            Scopes = tokenResponse.Scope ?? string.Empty
        };
    }

    /// <inheritdoc />
    public async Task<OAuthTokenResult> ExchangeMicrosoftCodeAsync(string code, string redirectUri)
    {
        ArgumentException.ThrowIfNullOrEmpty(code);
        ArgumentException.ThrowIfNullOrEmpty(redirectUri);

        _logger.LogDebug("Exchanging Microsoft authorization code for tokens");

        var client = _httpClientFactory.CreateClient("OAuth");

        var requestBody = new FormUrlEncodedContent(new Dictionary<string, string>
        {
            ["code"] = code,
            ["client_id"] = _authSettings.Microsoft.ClientId,
            ["client_secret"] = _authSettings.Microsoft.ClientSecret,
            ["redirect_uri"] = redirectUri,
            ["grant_type"] = "authorization_code",
            ["scope"] = "openid profile email offline_access https://graph.microsoft.com/Mail.Read"
        });

        var tokenEndpoint = _authSettings.Microsoft.TokenEndpoint;
        var response = await client.PostAsync(tokenEndpoint, requestBody);

        if (!response.IsSuccessStatusCode)
        {
            var errorBody = await response.Content.ReadAsStringAsync();
            _logger.LogError(
                "Microsoft token exchange failed with status {StatusCode}: {ErrorBody}",
                response.StatusCode, errorBody);
            throw new AuthenticationException("Failed to exchange Microsoft authorization code for tokens.");
        }

        var tokenResponse = await response.Content.ReadFromJsonAsync<MicrosoftTokenResponse>(JsonOptions);

        if (tokenResponse is null || string.IsNullOrEmpty(tokenResponse.AccessToken))
        {
            throw new AuthenticationException("Microsoft token response was empty or malformed.");
        }

        _logger.LogInformation("Successfully exchanged Microsoft authorization code for tokens");

        return new OAuthTokenResult
        {
            AccessToken = tokenResponse.AccessToken,
            RefreshToken = tokenResponse.RefreshToken,
            IdToken = tokenResponse.IdToken,
            ExpiresInSeconds = tokenResponse.ExpiresIn,
            Scopes = tokenResponse.Scope ?? string.Empty
        };
    }

    /// <inheritdoc />
    public async Task<OAuthUserInfo> GetGoogleUserInfoAsync(string accessToken)
    {
        ArgumentException.ThrowIfNullOrEmpty(accessToken);

        _logger.LogDebug("Retrieving Google user profile");

        var client = _httpClientFactory.CreateClient("OAuth");
        client.DefaultRequestHeaders.Authorization = new AuthenticationHeaderValue("Bearer", accessToken);

        var response = await client.GetAsync(_authSettings.Google.UserInfoEndpoint);

        if (!response.IsSuccessStatusCode)
        {
            var errorBody = await response.Content.ReadAsStringAsync();
            _logger.LogError(
                "Google user info request failed with status {StatusCode}: {ErrorBody}",
                response.StatusCode, errorBody);
            throw new AuthenticationException("Failed to retrieve user profile from Google.");
        }

        var userInfo = await response.Content.ReadFromJsonAsync<GoogleUserInfoResponse>(JsonOptions);

        if (userInfo is null || string.IsNullOrEmpty(userInfo.Email))
        {
            throw new AuthenticationException("Google user profile response was empty or missing email.");
        }

        _logger.LogInformation("Retrieved Google user profile for {Email}", userInfo.Email);

        return new OAuthUserInfo
        {
            ProviderUserId = userInfo.Id,
            Email = userInfo.Email,
            DisplayName = userInfo.Name ?? userInfo.Email
        };
    }

    /// <inheritdoc />
    public async Task<OAuthUserInfo> GetMicrosoftUserInfoAsync(string accessToken)
    {
        ArgumentException.ThrowIfNullOrEmpty(accessToken);

        _logger.LogDebug("Retrieving Microsoft user profile from Graph API");

        var client = _httpClientFactory.CreateClient("OAuth");
        client.DefaultRequestHeaders.Authorization = new AuthenticationHeaderValue("Bearer", accessToken);

        var response = await client.GetAsync(_authSettings.Microsoft.UserInfoEndpoint);

        if (!response.IsSuccessStatusCode)
        {
            var errorBody = await response.Content.ReadAsStringAsync();
            _logger.LogError(
                "Microsoft Graph user info request failed with status {StatusCode}: {ErrorBody}",
                response.StatusCode, errorBody);
            throw new AuthenticationException("Failed to retrieve user profile from Microsoft.");
        }

        var userInfo = await response.Content.ReadFromJsonAsync<MicrosoftUserInfoResponse>();

        if (userInfo is null || string.IsNullOrEmpty(userInfo.Id))
        {
            throw new AuthenticationException("Microsoft user profile response was empty or missing ID.");
        }

        // Microsoft Graph may return mail or userPrincipalName
        var email = userInfo.Mail ?? userInfo.UserPrincipalName ?? string.Empty;

        if (string.IsNullOrEmpty(email))
        {
            throw new AuthenticationException("Microsoft user profile is missing email address.");
        }

        _logger.LogInformation("Retrieved Microsoft user profile for {Email}", email);

        return new OAuthUserInfo
        {
            ProviderUserId = userInfo.Id,
            Email = email,
            DisplayName = userInfo.DisplayName ?? email
        };
    }

    #region Internal response DTOs

    private class GoogleTokenResponse
    {
        [JsonPropertyName("access_token")]
        public string AccessToken { get; init; } = string.Empty;

        [JsonPropertyName("refresh_token")]
        public string? RefreshToken { get; init; }

        [JsonPropertyName("id_token")]
        public string? IdToken { get; init; }

        [JsonPropertyName("expires_in")]
        public int ExpiresIn { get; init; }

        [JsonPropertyName("scope")]
        public string? Scope { get; init; }

        [JsonPropertyName("token_type")]
        public string? TokenType { get; init; }
    }

    private class MicrosoftTokenResponse
    {
        [JsonPropertyName("access_token")]
        public string AccessToken { get; init; } = string.Empty;

        [JsonPropertyName("refresh_token")]
        public string? RefreshToken { get; init; }

        [JsonPropertyName("id_token")]
        public string? IdToken { get; init; }

        [JsonPropertyName("expires_in")]
        public int ExpiresIn { get; init; }

        [JsonPropertyName("scope")]
        public string? Scope { get; init; }

        [JsonPropertyName("token_type")]
        public string? TokenType { get; init; }
    }

    private class GoogleUserInfoResponse
    {
        [JsonPropertyName("id")]
        public string Id { get; init; } = string.Empty;

        [JsonPropertyName("email")]
        public string Email { get; init; } = string.Empty;

        [JsonPropertyName("name")]
        public string? Name { get; init; }

        [JsonPropertyName("picture")]
        public string? Picture { get; init; }

        [JsonPropertyName("verified_email")]
        public bool VerifiedEmail { get; init; }
    }

    private class MicrosoftUserInfoResponse
    {
        [JsonPropertyName("id")]
        public string Id { get; init; } = string.Empty;

        [JsonPropertyName("displayName")]
        public string? DisplayName { get; init; }

        [JsonPropertyName("mail")]
        public string? Mail { get; init; }

        [JsonPropertyName("userPrincipalName")]
        public string? UserPrincipalName { get; init; }
    }

    #endregion
}
