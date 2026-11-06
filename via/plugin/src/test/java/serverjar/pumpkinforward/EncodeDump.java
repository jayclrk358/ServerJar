package serverjar.pumpkinforward;

import com.mojang.authlib.properties.Property;
import io.netty.buffer.ByteBuf;
import io.netty.buffer.Unpooled;

import java.nio.charset.StandardCharsets;
import java.util.List;
import java.util.UUID;

/** Prints the encoded forwarding packet as hex so test/check.py can parse it like Pumpkin does. */
public final class EncodeDump {
    public static void main(String[] args) throws Exception {
        ByteBuf out = Unpooled.buffer();
        VelocityForwardHandler.encode(out, 123456, "2001:db8::1",
                UUID.fromString("069a79f4-44e9-4726-a5be-fca90e38aaf5"), "Notch",
                List.of(new Property("textures", "dGV4dHVyZXM=", "c2ln"), new Property("other", "x", null)),
                "s3cret".getBytes(StandardCharsets.UTF_8));
        byte[] b = new byte[out.readableBytes()];
        out.readBytes(b);
        StringBuilder sb = new StringBuilder();
        for (byte x : b) sb.append(String.format("%02x", x));
        System.out.println(sb);
    }
}
