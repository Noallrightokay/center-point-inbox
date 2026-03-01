using System.ComponentModel.DataAnnotations;
using System.Net;
using System.Security.Authentication;
using System.Text.Json;
using CenterPointInbox.Shared.Models;
using FluentValidation;
using Microsoft.AspNetCore.Http;
using Microsoft.Extensions.Logging;

namespace CenterPointInbox.Shared.Middleware;

/// <summary>
/// Global exception handling middleware that catches unhandled exceptions,
/// logs them with the correlation ID, and returns a structured ApiResponse error.
/// Maps known exception types to appropriate HTTP status codes.
/// </summary>
public class ExceptionHandlingMiddleware
{
    private readonly RequestDelegate _next;
    private readonly ILogger<ExceptionHandlingMiddleware> _logger;

    private static readonly JsonSerializerOptions JsonOptions = new()
    {
        PropertyNamingPolicy = JsonNamingPolicy.CamelCase,
        DefaultIgnoreCondition = System.Text.Json.Serialization.JsonIgnoreCondition.WhenWritingNull
    };

    public ExceptionHandlingMiddleware(RequestDelegate next, ILogger<ExceptionHandlingMiddleware> logger)
    {
        _next = next;
        _logger = logger;
    }

    public async Task InvokeAsync(HttpContext context)
    {
        try
        {
            await _next(context);
        }
        catch (Exception ex)
        {
            await HandleExceptionAsync(context, ex);
        }
    }

    private async Task HandleExceptionAsync(HttpContext context, Exception exception)
    {
        var correlationId = context.GetCorrelationId() ?? Guid.NewGuid().ToString("D");
        var (statusCode, errorCode, message) = MapException(exception);

        _logger.LogError(
            exception,
            "Unhandled exception occurred. CorrelationId: {CorrelationId}, StatusCode: {StatusCode}, ErrorCode: {ErrorCode}, Path: {Path}",
            correlationId,
            (int)statusCode,
            errorCode,
            context.Request.Path);

        context.Response.ContentType = "application/json";
        context.Response.StatusCode = (int)statusCode;

        // Ensure correlation ID header is present
        if (!context.Response.Headers.ContainsKey(CorrelationIdMiddleware.HeaderName))
        {
            context.Response.Headers[CorrelationIdMiddleware.HeaderName] = correlationId;
        }

        ApiResponse<object> response;

        if (exception is FluentValidation.ValidationException validationException)
        {
            var validationErrors = validationException.Errors
                .GroupBy(e => e.PropertyName)
                .ToDictionary(
                    g => g.Key,
                    g => g.Select(e => e.ErrorMessage).ToArray()
                ) as IDictionary<string, string[]>;

            response = ApiResponse<object>.Error(message, validationErrors, correlationId);
        }
        else
        {
            response = ApiResponse<object>.Error(message, errorCode, (int)statusCode, correlationId);
        }

        var json = JsonSerializer.Serialize(response, JsonOptions);
        await context.Response.WriteAsync(json);
    }

    private static (HttpStatusCode StatusCode, string ErrorCode, string Message) MapException(Exception exception)
    {
        return exception switch
        {
            FluentValidation.ValidationException ex =>
                (HttpStatusCode.UnprocessableEntity, "VALIDATION_ERROR", "One or more validation errors occurred."),

            System.ComponentModel.DataAnnotations.ValidationException ex =>
                (HttpStatusCode.UnprocessableEntity, "VALIDATION_ERROR", ex.Message),

            UnauthorizedAccessException ex =>
                (HttpStatusCode.Unauthorized, "UNAUTHORIZED", ex.Message.Length > 0 ? ex.Message : "Authentication is required."),

            AuthenticationException ex =>
                (HttpStatusCode.Unauthorized, "AUTHENTICATION_FAILED", ex.Message.Length > 0 ? ex.Message : "Authentication failed."),

            InvalidOperationException ex when ex.Message.Contains("forbidden", StringComparison.OrdinalIgnoreCase) =>
                (HttpStatusCode.Forbidden, "FORBIDDEN", "You do not have permission to perform this action."),

            KeyNotFoundException ex =>
                (HttpStatusCode.NotFound, "NOT_FOUND", ex.Message.Length > 0 ? ex.Message : "The requested resource was not found."),

            FileNotFoundException ex =>
                (HttpStatusCode.NotFound, "NOT_FOUND", ex.Message.Length > 0 ? ex.Message : "The requested file was not found."),

            ArgumentException ex =>
                (HttpStatusCode.BadRequest, "BAD_REQUEST", ex.Message),

            FormatException ex =>
                (HttpStatusCode.BadRequest, "BAD_REQUEST", ex.Message),

            NotSupportedException ex =>
                (HttpStatusCode.BadRequest, "NOT_SUPPORTED", ex.Message),

            NotImplementedException =>
                (HttpStatusCode.NotImplemented, "NOT_IMPLEMENTED", "This feature is not yet implemented."),

            TimeoutException =>
                (HttpStatusCode.GatewayTimeout, "TIMEOUT", "The operation timed out. Please try again."),

            OperationCanceledException =>
                (HttpStatusCode.BadRequest, "REQUEST_CANCELLED", "The request was cancelled."),

            HttpRequestException ex =>
                (HttpStatusCode.BadGateway, "UPSTREAM_ERROR", "An error occurred communicating with an upstream service."),

            _ =>
                (HttpStatusCode.InternalServerError, "INTERNAL_ERROR", "An unexpected error occurred. Please try again later.")
        };
    }
}
