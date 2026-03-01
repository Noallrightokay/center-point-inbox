using System.Text.Json.Serialization;

namespace CenterPointInbox.Shared.Models;

public class ApiResponse<T>
{
    [JsonPropertyName("success")]
    public bool IsSuccess { get; init; }

    [JsonPropertyName("data")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public T? Data { get; init; }

    [JsonPropertyName("error")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public ApiError? Error { get; init; }

    [JsonPropertyName("pagination")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public PaginationMeta? Pagination { get; init; }

    [JsonPropertyName("correlationId")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public string? CorrelationId { get; init; }

    [JsonPropertyName("timestamp")]
    public DateTimeOffset Timestamp { get; init; } = DateTimeOffset.UtcNow;

    public static ApiResponse<T> Success(T data, string? correlationId = null)
    {
        return new ApiResponse<T>
        {
            IsSuccess = true,
            Data = data,
            CorrelationId = correlationId
        };
    }

    public static ApiResponse<T> Success(T data, PaginationMeta pagination, string? correlationId = null)
    {
        return new ApiResponse<T>
        {
            IsSuccess = true,
            Data = data,
            Pagination = pagination,
            CorrelationId = correlationId
        };
    }

    public static ApiResponse<T> Error(string message, string? code = null, int? statusCode = null, string? correlationId = null)
    {
        return new ApiResponse<T>
        {
            IsSuccess = false,
            Error = new ApiError
            {
                Message = message,
                Code = code,
                StatusCode = statusCode
            },
            CorrelationId = correlationId
        };
    }

    public static ApiResponse<T> Error(string message, IDictionary<string, string[]> validationErrors, string? correlationId = null)
    {
        return new ApiResponse<T>
        {
            IsSuccess = false,
            Error = new ApiError
            {
                Message = message,
                Code = "VALIDATION_ERROR",
                StatusCode = 422,
                ValidationErrors = validationErrors
            },
            CorrelationId = correlationId
        };
    }
}

public class ApiResponse : ApiResponse<object>
{
    public static ApiResponse Success(string? correlationId = null)
    {
        return new ApiResponse
        {
            IsSuccess = true,
            CorrelationId = correlationId
        };
    }

    public new static ApiResponse Error(string message, string? code = null, int? statusCode = null, string? correlationId = null)
    {
        return new ApiResponse
        {
            IsSuccess = false,
            Error = new ApiError
            {
                Message = message,
                Code = code,
                StatusCode = statusCode
            },
            CorrelationId = correlationId
        };
    }
}

public class ApiError
{
    [JsonPropertyName("message")]
    public required string Message { get; init; }

    [JsonPropertyName("code")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public string? Code { get; init; }

    [JsonPropertyName("statusCode")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public int? StatusCode { get; init; }

    [JsonPropertyName("validationErrors")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public IDictionary<string, string[]>? ValidationErrors { get; init; }
}

public class PaginationMeta
{
    [JsonPropertyName("page")]
    public int Page { get; init; }

    [JsonPropertyName("pageSize")]
    public int PageSize { get; init; }

    [JsonPropertyName("totalCount")]
    public long TotalCount { get; init; }

    [JsonPropertyName("totalPages")]
    public int TotalPages { get; init; }

    [JsonPropertyName("hasNextPage")]
    public bool HasNextPage => Page < TotalPages;

    [JsonPropertyName("hasPreviousPage")]
    public bool HasPreviousPage => Page > 1;

    public static PaginationMeta Create(int page, int pageSize, long totalCount)
    {
        return new PaginationMeta
        {
            Page = page,
            PageSize = pageSize,
            TotalCount = totalCount,
            TotalPages = (int)Math.Ceiling(totalCount / (double)pageSize)
        };
    }
}
