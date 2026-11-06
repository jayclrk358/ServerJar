/*
 * PumpkinForward: a ViaProxy plugin that forwards the real player identity
 * (Mojang UUID, skin, IP) to a Pumpkin backend using Velocity modern forwarding.
 *
 * Without this, a Pumpkin server behind ViaProxy has to run in offline mode and
 * every player gets a name-based offline UUID and no skin. With it, Pumpkin's
 * `networking.proxy.velocity` support receives the profile ViaProxy verified
 * with Mojang, signed with a shared secret so nobody can forge it.
 *
 * Licensed under GPL-3.0, like the rest of ServerJar.
 */
package serverjar.pumpkinforward;

import net.lenni0451.lambdaevents.EventHandler;
import net.raphimc.viaproxy.ViaProxy;
import net.raphimc.viaproxy.plugins.ViaProxyPlugin;
import net.raphimc.viaproxy.plugins.events.Proxy2ServerChannelInitializeEvent;
import net.raphimc.viaproxy.plugins.events.types.ITyped;
import net.raphimc.viaproxy.protocoltranslator.impl.ViaProxyViaCodec;
import net.raphimc.viaproxy.util.logging.Logger;

import java.io.File;
import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.security.SecureRandom;
import java.util.Base64;

public class PumpkinForwardPlugin extends ViaProxyPlugin {

    /** Name of our netty handler in the proxy -> server pipeline. */
    public static final String HANDLER_NAME = "pumpkin-forward";
    /** Environment variable that overrides the secret file. */
    public static final String SECRET_ENV = "PUMPKIN_FORWARD_SECRET";
    /** File (inside this plugin's data folder) holding the shared secret. */
    public static final String SECRET_FILE = "secret.txt";

    private byte[] secret;

    @Override
    public void onEnable() {
        this.secret = this.loadSecret();
        if (this.secret == null) {
            Logger.LOGGER.error("[PumpkinForward] No forwarding secret configured; the plugin is disabled.");
            return;
        }
        ViaProxy.EVENT_MANAGER.register(this);
        Logger.LOGGER.info("[PumpkinForward] Enabled. Forwarding player identity to the backend via velocity:player_info.");
        if (!ViaProxy.getConfig().isProxyOnlineMode()) {
            Logger.LOGGER.warn("[PumpkinForward] proxy-online-mode is false: players are NOT verified with Mojang, so the forwarded UUIDs are offline UUIDs and anyone can join under any name.");
        }
    }

    @EventHandler
    public void onProxy2ServerInit(final Proxy2ServerChannelInitializeEvent event) {
        if (event.getType() != ITyped.Type.POST || event.isLegacyPassthrough()) {
            return;
        }
        // Insert before ViaProxy's protocol translator so we see (and answer) the backend's
        // raw login plugin request. The translator would otherwise drop it for pre-1.13 clients.
        if (event.getChannel().pipeline().get(ViaProxyViaCodec.NAME) != null) {
            event.getChannel().pipeline().addBefore(ViaProxyViaCodec.NAME, HANDLER_NAME, new VelocityForwardHandler(this.secret));
        }
    }

    private byte[] loadSecret() {
        final String fromEnv = System.getenv(SECRET_ENV);
        if (fromEnv != null && !fromEnv.isBlank()) {
            return fromEnv.trim().getBytes(StandardCharsets.UTF_8);
        }
        final File file = new File(this.getDataFolder(), SECRET_FILE);
        try {
            if (file.isFile()) {
                final String content = Files.readString(file.toPath(), StandardCharsets.UTF_8).trim();
                if (!content.isEmpty()) {
                    return content.getBytes(StandardCharsets.UTF_8);
                }
            }
            // First start: create a random secret so the server owner only has to copy it.
            final byte[] random = new byte[24];
            new SecureRandom().nextBytes(random);
            final String generated = Base64.getUrlEncoder().withoutPadding().encodeToString(random);
            Files.writeString(file.toPath(), generated + System.lineSeparator(), StandardCharsets.UTF_8);
            Logger.LOGGER.warn("[PumpkinForward] Generated a new secret in " + file.getPath());
            Logger.LOGGER.warn("[PumpkinForward] Put the same value into pumpkin.toml under [networking.proxy.velocity] secret = \"...\"");
            return generated.getBytes(StandardCharsets.UTF_8);
        } catch (IOException e) {
            Logger.LOGGER.error("[PumpkinForward] Could not read or write " + file.getPath(), e);
            return null;
        }
    }

}
