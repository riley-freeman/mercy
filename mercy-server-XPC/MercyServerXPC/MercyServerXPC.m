#import <Foundation/Foundation.h>
#include <mach/mach.h>

#import "MercyServerXPC.h"

// ──────────────────────────────────────────────
// Allocation tracking
// ──────────────────────────────────────────────

typedef struct {
    void *ptr;
    size_t size;
    xpc_object_t shmem; // lazily created on MapId
} Allocation;

static NSMutableDictionary<NSNumber *, NSValue *> *allocations;
static NSInteger nextAllocId = 1;

// ──────────────────────────────────────────────
// Helpers
// ──────────────────────────────────────────────

/// Encode an NSInteger allocation key into a 16-byte alloc_id (native endian, zero-padded).
static void encode_alloc_id(NSInteger key, uint8_t out[16]) {
    memset(out, 0, 16);
    memcpy(out, &key, sizeof(NSInteger));
}

/// Decode a 16-byte alloc_id back to an NSInteger key.
static NSInteger decode_alloc_id(const uint8_t *bytes, size_t len) {
    NSInteger key = 0;
    size_t copyLen = (len < sizeof(NSInteger)) ? len : sizeof(NSInteger);
    memcpy(&key, bytes, copyLen);
    return key;
}

/// Build an XPC reply dictionary with the standard envelope fields.
static xpc_object_t create_reply_envelope(int64_t originalId, const char *messageType) {
    xpc_object_t reply = xpc_dictionary_create(NULL, NULL, 0);
    xpc_dictionary_set_int64(reply, "id", originalId);
    xpc_dictionary_set_int64(reply, "reply_id", originalId);
    xpc_dictionary_set_string(reply, "message_type", messageType);
    return reply;
}

// ──────────────────────────────────────────────
// Message handlers
// ──────────────────────────────────────────────

static void handle_alloc(xpc_connection_t conn, int64_t msgId, xpc_object_t data) {
    int64_t size = xpc_dictionary_get_int64(data, "size");
    if (size <= 0) return;

    // Allocate page-aligned memory suitable for xpc_shmem
    vm_address_t address = 0;
    kern_return_t kr = vm_allocate(mach_task_self(), &address, (vm_size_t)size, VM_FLAGS_ANYWHERE);
    if (kr != KERN_SUCCESS) return;

    // Store the allocation
    NSInteger allocId = nextAllocId++;
    Allocation alloc = { .ptr = (void *)address, .size = (size_t)size, .shmem = NULL };
    allocations[@(allocId)] = [NSValue valueWithBytes:&alloc objCType:@encode(Allocation)];

    // Build and send reply
    xpc_object_t reply = create_reply_envelope(msgId, "Alloc");
    xpc_object_t replyData = xpc_dictionary_create(NULL, NULL, 0);

    uint8_t allocIdBytes[16];
    encode_alloc_id(allocId, allocIdBytes);
    xpc_dictionary_set_data(replyData, "alloc_id", allocIdBytes, 16);

    xpc_dictionary_set_value(reply, "message_data", replyData);
    xpc_connection_send_message(conn, reply);

    // xpc_release(replyData);
    // xpc_release(reply);
}

static void handle_free(int64_t msgId, xpc_object_t data) {
    size_t len = 0;
    const void *allocIdBytes = xpc_dictionary_get_data(data, "alloc_id", &len);
    if (!allocIdBytes || len == 0) return;

    NSInteger allocId = decode_alloc_id(allocIdBytes, len);
    NSValue *val = allocations[@(allocId)];
    if (!val) return;

    Allocation alloc;
    [val getValue:&alloc];

    // Deallocate the memory region
    vm_deallocate(mach_task_self(), (vm_address_t)alloc.ptr, alloc.size);

    // Release the shmem object if it was created
    if (alloc.shmem) {
        // xpc_release(alloc.shmem);
    }

    [allocations removeObjectForKey:@(allocId)];
}

static void handle_map_id(xpc_connection_t conn, int64_t msgId, xpc_object_t data) {
    size_t len = 0;
    const void *allocIdBytes = xpc_dictionary_get_data(data, "alloc_id", &len);
    if (!allocIdBytes || len == 0) return;

    NSInteger allocId = decode_alloc_id(allocIdBytes, len);
    NSValue *val = allocations[@(allocId)];
    if (!val) return;

    Allocation alloc;
    [val getValue:&alloc];

    // Lazily create the xpc_shmem object
    if (!alloc.shmem) {
        alloc.shmem = xpc_shmem_create(alloc.ptr, alloc.size);
        // Update the stored allocation with the shmem reference
        allocations[@(allocId)] = [NSValue valueWithBytes:&alloc objCType:@encode(Allocation)];
    }

    // Build and send reply — package the shmem as a uint64
    xpc_object_t reply = create_reply_envelope(msgId, "MapId");
    xpc_object_t replyData = xpc_dictionary_create(NULL, NULL, 0);

    xpc_dictionary_set_uint64(replyData, "xpc_handle_int", (uint64_t)alloc.shmem);

    xpc_dictionary_set_value(reply, "message_data", replyData);
    xpc_connection_send_message(conn, reply);

    // xpc_release(replyData);
    // xpc_release(reply);
}

// ──────────────────────────────────────────────
// Event handler
// ──────────────────────────────────────────────

void xpc_event_handler(xpc_connection_t conn, xpc_object_t obj) {
    // Ignore non-dictionary objects (errors, etc.)
    if (xpc_get_type(obj) != XPC_TYPE_DICTIONARY) return;

    int64_t msgId = xpc_dictionary_get_int64(obj, "id");
    const char *messageType = xpc_dictionary_get_string(obj, "message_type");
    xpc_object_t data = xpc_dictionary_get_value(obj, "message_data");

    if (!messageType || !data) return;

    if (strcmp(messageType, "Alloc") == 0) {
        handle_alloc(conn, msgId, data);
    } else if (strcmp(messageType, "Free") == 0) {
        handle_free(msgId, data);
    } else if (strcmp(messageType, "MapId") == 0) {
        handle_map_id(conn, msgId, data);
    }
}

void xpc_connection_handler(xpc_connection_t conn) {
    xpc_connection_set_event_handler(conn, ^void(xpc_object_t obj) {
        xpc_event_handler(conn, obj);
    });
    xpc_connection_resume(conn);
}

int mercy_server_xpc_main(int argc, const char *argv[])
{
    allocations = [NSMutableDictionary new];
    xpc_main(xpc_connection_handler);
    return 0;
}
